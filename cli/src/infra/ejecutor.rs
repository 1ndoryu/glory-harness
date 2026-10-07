//! Ejecutor real de comandos del CLI (318A-16 F3).
//!
//! Implementa el puerto `EjecutorComando` del núcleo con tokio `Command`:
//! ejecución DIRECTA sin shell (jaula `infra::jaula`, 139A-8 F1/K1), timeout
//! acotado, truncado de salida a 8 KB y tareas de fondo identificadas por id
//! (`comando_status`/`comando_matar`). El núcleo queda agnóstico: solo ve este
//! trait; un consumidor sin runner (p. ej. PROYECTO TASKS, que deniega
//! comandos) no registra la tool en absoluto (fail-closed).
//!
//! [119A-7 F0] Jaula: el ejecutor puede fijar el directorio de arranque de
//! cada hijo (`en_raiz`). Los comandos heredan ese cwd, así que las rutas
//! relativas del modelo caen dentro del workspace del run. Límite honesto:
//! el cwd no contiene `..` absolutos; la contención total la dan la jaula
//! (sin shell + allowlist/denylist) + clasificación de riesgo
//! (`bash_clasificar`) + aprobación + supervisión. `nuevo()` (sin raíz,
//! hereda el cwd del proceso) queda solo para diagnósticos sin run.
//!
//! [139A-8 F1/K1] Sin shell: `ejecutar`/`ejecutar_fondo` construyen con
//! `jaula::construir_directo` (argv directo, builtins `cmd` con veto en
//! Windows). Tuberías/redirecciones/`&&`/`$()` se DENIEGAN con mensaje
//! claro (cambio de conducta documentado en `jaula.rs`).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::{Duration, Instant};

use tokio::process::{Child, ChildStdin};
use tokio::sync::{Mutex, Notify};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use glory_harness_core::ports::{ChunkConsola, OrigenConsola, ResultadoEjecucionComando};

/// Límite de salida capturada por comando (8 KB, contrato del plan 318A-16).
const LIMITE_SALIDA: usize = 8 * 1024;
/// Timeout por comando síncrono: 120 s (un comando colgado no bloquea el turno).
const TIMEOUT_COMANDO: Duration = Duration::from_secs(120);
/// [209A-1 F1] Tope de stream en vivo por ejecución (64 KB): pasado el tope
/// el pump deja de reenviar líneas pero SIGUE acumulando para el resultado
/// final de 8 KB. Anti-fuga mínimo (un `yes` infinito no hincha el canal ni
/// el SSE); F2 formaliza los topes de consola viva (ring + transcript).
const LIMITE_STREAM_BYTES: usize = 64 * 1024;
/// [209A-1 F1] Línea de stream recortada a 2 KB en chars (barras de progreso
/// con `\r` sin `\n` no hinchan el canal; el resultado final conserva su
/// propio truncado a 8 KB).
const LIMITE_LINEA_STREAM: usize = 2048;
/// [209A-1 F2] Máximo de consolas vivas simultáneas: la 5ª ejecución
/// desacoplada se RECHAZA (`Error::Ocupado`) en vez de ejecutarse sin
/// visor. Anti-fuga: sin tope, un modelo podría lanzar N builds en fondo.
const MAX_CONSOLAS_VIVAS: usize = 4;
/// [209A-1 F2] Ring por consola viva (128 KB): el hilo pump retiene lo
/// último para `comando_lista`/reconexión; al superar el tope se descartan
/// las líneas más antiguas (contador `bytes_descartados` en el fin).
const LIMITE_RING_BYTES: usize = 128 * 1024;
/// [209A-1 F2] Máximo de consolas recientes recordadas: el mapa `resultados`
/// no crece sin cota (una sesión larga con cientos de ejecuciones no hincha
/// la memoria del proceso CLI/web).
const MAX_CONSOLAS_RECIENTES: usize = 64;
/// [219A-3] Tope por escritura al stdin de una consola viva (64 KB): una
/// pegada accidental no hincha la tubería; el llamador trocea si necesita más.
const MAX_ESCRITURA_STDIN: usize = 64 * 1024;
/// [219A-3] Tope de líneas del transcript de `salida` (backfill de la UI al
/// abrir la tab): la UI pide el vivo + este volcado acotado, no el ring entero.
const MAX_LINEAS_TRANSCRIPT: usize = 2000;

/// [209A-1 F2] Consola viva: metadatos + anillo de una ejecución desacoplada
/// en curso. El hijo lo posee la tarea pump (`tareas`, como en F1: quien lo
/// toma —pump o `matar`— gana); la viva solo retiene lo visible para
/// `comando_lista`/el tab Consola. El pump la retira de `vivas` al salir el
/// hijo y archiva en `resultados` (reap automático); `matar` mata al hijo y
/// el pump hace el resto. `desacoplar` marca `suelta` para que el pump deje
/// de intentar el envío al turno (sigue acumulando + anillo).
struct ConsolaViva {
    comando: String,
    conversacion_id: Uuid,
    inicio: Instant,
    /// [219A-4] Dueño (agente por defecto; `Usuario` en consolas propias).
    origen: OrigenConsola,
    /// Señal de `desacoplar`: el pump suelta el envío al turno.
    suelta: AtomicBool,
    /// [209A-1 F4] Señal de kill tardío: si `matar` llega cuando el pump ya
    /// tomó el hijo (el handle quedó en `None`), el pump lo mata él mismo
    /// (es el único que posee el `Child` en ese momento).
    matar: Notify,
    /// Anillo de líneas recientes (capado a `LIMITE_RING_BYTES`).
    /// [219A-3] Guarda el chunk con su flujo: el transcript de `salida` lo
    /// necesita para el backfill de la UI (stdout vs stderr).
    anillo: Mutex<VecDeque<ChunkConsola>>,
    bytes_anillo: AtomicUsize,
    bytes_descartados: AtomicUsize,
    /// [219A-3] Stdin del hijo para `escribir` (interactuar desde la UI).
    /// El pump nunca lo toca; `None` cuando el hijo ya salió (el `Child` lo
    /// posee la tarea pump y al salir no hay a quién escribir).
    stdin: Mutex<Option<ChildStdin>>,
}

/// [219A-4] Parámetros del registro común `registrar_fondo` (un solo
/// registro + pump para el modelo y el operador; el struct evita el
/// `too_many_arguments` de clippy). El hijo ya viene spawneado.
struct RegistroFondo {
    id: String,
    comando: String,
    conversacion_id: Uuid,
    chunks: Option<UnboundedSender<ChunkConsola>>,
    origen: OrigenConsola,
    hijo_inicial: Child,
    stdin_hijo: Option<ChildStdin>,
}

/// [219A-5 F3] Archivada con dueño: el resultado + el origen de la viva.
/// `lista`/`salida` etiquetan con el dueño real (antes forzaban `Agente`).
struct Archivada {
    resultado: ResultadoEjecucionComando,
    origen: OrigenConsola,
}

/// [219A-5 F2] Registro único de consolas bajo UN solo lock: vivas +
/// archivadas + orden de archivo (fin real). El reap (retirar viva +
/// archivar) es atómico: ningún lector ve el id en tierra de nadie.
/// `orden` fija el orden de `lista` y la evicción saca la MÁS ANTIGUA
/// (antes: `Instant::now()` sobre un HashMap y evicción arbitraria).
#[derive(Default)]
struct RegistroConsolas {
    vivas: HashMap<String, Arc<ConsolaViva>>,
    archivadas: HashMap<String, Archivada>,
    orden: VecDeque<String>,
}

/// Handle compartido de una tarea de fondo: el spawner y `matar` compiten por
/// el `Child`; quien lo toma (o mata) lo deja en `None`.
type HandleTarea = Arc<Mutex<Option<Child>>>;

/// Implementación concreta del puerto para el CLI.
pub struct EjecutorCliente {
    tareas: Arc<Mutex<HashMap<String, HandleTarea>>>,
    /// [219A-5 F2] Vivas + archivadas + orden bajo un solo lock (reap
    /// atómico, orden de fin real, evicción de la más antigua).
    registro: Arc<Mutex<RegistroConsolas>>,
    /// [119A-7 F0] Raíz enjaulada: cwd de arranque de cada hijo.
    /// `None` = heredar el cwd del proceso (solo diagnósticos sin run).
    raiz: Option<PathBuf>,
}

impl EjecutorCliente {
    #[must_use]
    pub fn nuevo() -> Self {
        Self {
            tareas: Arc::default(),
            registro: Arc::default(),
            raiz: None,
        }
    }

    /// Ejecutor enjaulado: cada comando arranca con cwd = `raiz`.
    #[must_use]
    pub fn en_raiz(raiz: PathBuf) -> Self {
        Self {
            tareas: Arc::default(),
            registro: Arc::default(),
            raiz: Some(raiz),
        }
    }
}

// Submódulos por dominio (partición 309A-3): flujo (construcción sin
// shell, bomba, anillo y truncado), fondo (spawn detached + pump con
// tope de vivas), sincrono (rama con timeout) y consultas (puerto
// EjecutorComando + reap). Los bloques impl viven ahí; aquí quedan
// tipos, consts, constructores y tests.
mod consultas;
mod flujo;
mod fondo;
mod sincrono;

#[cfg(test)]
mod tests {
    use super::*;
    use glory_harness_core::error::Error;
    use glory_harness_core::ports::EjecutorComando;

    fn comando_lento(segundos: u64) -> String {
        if cfg!(windows) {
            // `ping -n N` en Windows espera ~N-1 segundos y sale con código 0.
            // [139A-8 F1/K1] Sin `>nul`: la redirección la deniega la jaula
            // (la salida capturada la archiva el ejecutor igualmente).
            format!("ping -n {} 127.0.0.1", segundos + 1)
        } else {
            format!("sleep {segundos}")
        }
    }

    fn comando_mucho_eco() -> String {
        if cfg!(windows) {
            // [139A-8 F1/K1] El `for /L … do @echo` es sintaxis cmd y la
            // jaula lo deniega: `dir` de System32 (~5000 entradas, >200 KB)
            // fuerza el truncado en segundos sin shell ni redirección.
            r"dir C:\Windows\System32".to_string()
        } else {
            // Sin tubería (la jaula la deniega): 90 KB de NULes bastan para
            // forzar el truncado a 8 KB.
            "head -c 90000 /dev/zero".to_string()
        }
    }

    #[tokio::test]
    async fn sincrono_devuelve_salida_y_codigo() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar("echo hola-harness", false).await.unwrap();
        assert!(!r.fondo);
        assert_eq!(r.codigo_salida, Some(0));
        assert!(r.salida.contains("hola-harness"), "salida: {}", r.salida);
        assert!(!r.truncada);
    }

    #[tokio::test]
    async fn salida_larga_se_trunca_a_8kb() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_mucho_eco(), false).await.unwrap();
        assert_eq!(r.codigo_salida, Some(0));
        assert!(
            r.truncada,
            "salida inesperadamente corta: {} bytes",
            r.salida.len()
        );
        assert!(
            r.salida.len() <= LIMITE_SALIDA + 64,
            "longitud: {}",
            r.salida.len()
        );
        assert!(r.salida.contains("truncada"));
    }

    #[tokio::test]
    async fn fondo_devuelve_id_y_status_espera_resultado() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(3), true).await.unwrap();
        assert!(r.fondo);
        let id = r.id_fondo.expect("fondo debe devolver id");

        // Poll hasta que la tarea termine (máx. 15 s) y quede archivada.
        let mut resultado = None;
        for _ in 0..30 {
            let s = e.estado(&id).await.unwrap();
            if s.codigo_salida.is_some() {
                resultado = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let resultado = resultado.expect("la tarea de fondo debió terminar");
        assert_eq!(resultado.codigo_salida, Some(0));
        assert_eq!(resultado.id_fondo.as_deref(), Some(id.as_str()));
    }

    #[tokio::test]
    async fn matar_termina_la_tarea_de_fondo() {        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(60), true).await.unwrap();
        let id = r.id_fondo.expect("fondo debe devolver id");
        // Estado inmediato: debe seguir en ejecución (el comando dura ~60 s).
        let s = e.estado(&id).await.unwrap();
        assert!(
            s.codigo_salida.is_none(),
            "aún corriendo, salida: {}",
            s.salida
        );
        e.matar(&id).await.unwrap();
        // Tras matar, la tarea deja de estar "en ejecución" en ≤ 5 s.
        for _ in 0..10 {
            let s = e.estado(&id).await.unwrap();
            if !s.salida.contains("aún en ejecución") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("la tarea siguió reportando ejecución tras matar");
    }

    /// [209A-1 F2] La viva aparece en `lista()` con comando y conversación,
    /// y el pump la reapea al terminar (pasa a archivada).
    #[tokio::test]
    async fn fondo_registra_viva_visible_en_lista_y_reapea_al_terminar() {
        let e = EjecutorCliente::nuevo();
        let conv = Uuid::new_v4();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-viva-{}", Uuid::new_v4());
        let r = e
            .ejecutar_en_vivo(&id, &comando_lento(3), conv, true, tx)
            .await
            .unwrap();
        assert!(r.fondo);
        let vivas: Vec<_> =
            e.lista().await.unwrap().into_iter().filter(|i| i.viva).collect();
        assert_eq!(vivas.len(), 1, "una viva, lista: {:?}", e.lista().await.unwrap());
        assert_eq!(vivas[0].id_ejecucion, id);
        assert_eq!(vivas[0].conversacion_id, conv);
        assert!(vivas[0].comando.contains("ping") || vivas[0].comando.contains("sleep"));
        // Esperar el fin: la viva se reapea y queda archivada con código.
        let mut archivada = None;
        for _ in 0..30 {
            let infos = e.lista().await.unwrap();
            if infos.iter().all(|i| !i.viva) && !infos.is_empty() {
                archivada = infos.into_iter().find(|i| i.id_ejecucion == id);
                if archivada.is_some() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let archivada = archivada.expect("la viva debió reapearse al terminar");
        assert!(!archivada.viva);
        assert_eq!(archivada.codigo_salida, Some(0));
    }

    /// [219A-5 F2/F3] Reap atómico en orden de fin + dueño retenido: dos
    /// fondos en serie quedan archivados en orden de terminación (el
    /// registro único sustituye el `Instant::now()` sobre el HashMap), y
    /// una propia archiva como `usuario` (ya no `agente` a la fuerza).
    #[tokio::test]
    async fn reap_ordena_por_fin_y_retiene_dueno() {
        let e = EjecutorCliente::nuevo();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id1 = format!("test-orden-{}", Uuid::new_v4());
        e.ejecutar_en_vivo(&id1, "echo primero-219A-5", Uuid::new_v4(), true, tx)
            .await
            .unwrap();
        for _ in 0..60 {
            if e.estado(&id1).await.unwrap().codigo_salida.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // Propia: shell vivo (se mata para archivarla).
        let id2 = e.ejecutar_propia(None).await.unwrap();
        let viva_propia = e
            .lista()
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.id_ejecucion == id2)
            .expect("propia viva en lista");
        assert!(viva_propia.viva);
        assert_eq!(viva_propia.origen, OrigenConsola::Usuario);
        e.matar(&id2).await.unwrap();
        for _ in 0..60 {
            let infos = e.lista().await.unwrap();
            if infos.iter().any(|i| i.id_ejecucion == id2 && !i.viva) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let infos = e.lista().await.unwrap();
        // Orden de fin real: primero el echo, después la propia matada.
        let hechas: Vec<&str> = infos
            .iter()
            .filter(|i| !i.viva)
            .map(|i| i.id_ejecucion.as_str())
            .collect();
        assert_eq!(
            hechas,
            vec![id1.as_str(), id2.as_str()],
            "orden de archivo: {hechas:?}"
        );
        // Dueño retenido al archivar: la propia sigue siendo del usuario.
        let arch = infos.iter().find(|i| i.id_ejecucion == id2).unwrap();
        assert_eq!(arch.origen, OrigenConsola::Usuario);
        let t = e.salida(&id2).await.unwrap();
        assert!(!t.viva);
        assert_eq!(t.origen, OrigenConsola::Usuario);
    }

    /// [219A-3] `escribir` acepta bytes en una viva; `salida` vuelca el anillo
    /// (viva) o el resultado archivado; tras `matar`, stdin deja de existir.
    #[tokio::test]
    async fn escribir_acepta_en_viva_y_falla_tras_matar() {
        let e = EjecutorCliente::nuevo();
        let conv = Uuid::new_v4();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-stdin-{}", Uuid::new_v4());
        let r = e
            .ejecutar_en_vivo(&id, &comando_lento(25), conv, true, tx)
            .await
            .unwrap();
        assert!(r.fondo);
        let escritos = e.escribir(&id, b"hola\n").await.unwrap();
        assert_eq!(escritos, 5);
        let viva = e.salida(&id).await.unwrap();
        assert!(viva.viva);
        assert_eq!(viva.id_ejecucion, id);
        assert!(
            matches!(e.escribir("test-stdin-inexistente", b"x").await, Err(Error::NoEncontrado(_))),
            "id desconocido debe dar NoEncontrado"
        );
        e.matar(&id).await.unwrap();
        // Tras matar, el pump reapea y stdin deja de existir (≤ 5 s).
        let mut cerro = false;
        for _ in 0..10 {
            if matches!(e.escribir(&id, b"x").await, Err(Error::NoEncontrado(_))) {
                cerro = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(cerro, "stdin siguió aceptando tras matar");
        // Archivada: `salida` sigue disponible con el comando y sin viva.
        let fin = e.salida(&id).await.unwrap();
        assert!(!fin.viva);
        assert_eq!(fin.id_ejecucion, id);
    }

    /// [209A-1 F2] `desacoplar` no mata: la viva sigue corriendo y visible.
    #[tokio::test]
    async fn desacoplar_mantiene_la_viva_en_marcha() {
        let e = EjecutorCliente::nuevo();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-suelta-{}", Uuid::new_v4());
        e.ejecutar_en_vivo(&id, &comando_lento(60), Uuid::nil(), true, tx)
            .await
            .unwrap();
        e.desacoplar(&id).await.unwrap();
        // Sigue viva tras desacoplar…
        let sigue = e.lista().await.unwrap().into_iter().find(|i| i.id_ejecucion == id);
        assert!(sigue.is_some_and(|i| i.viva), "desacoplar no mata");
        // …y se puede matar igual (el pump archiva).
        e.matar(&id).await.unwrap();
        for _ in 0..10 {
            let s = e.estado(&id).await.unwrap();
            if !s.salida.contains("aún en ejecución") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("la viva siguió corriendo tras matar");
    }

    /// [209A-1 F2] El tope `MAX_CONSOLAS_VIVAS` rechaza la 5ª sin zombis.
    #[tokio::test]
    async fn tope_de_vivas_rechaza_la_quinta() {
        let e = EjecutorCliente::nuevo();
        let mut ids = Vec::new();
        for _ in 0..MAX_CONSOLAS_VIVAS {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let id = format!("test-tope-{}", Uuid::new_v4());
            e.ejecutar_en_vivo(&id, &comando_lento(60), Uuid::nil(), true, tx)
                .await
                .unwrap();
            ids.push(id);
        }
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let err = e
            .ejecutar_en_vivo("test-tope-extra", &comando_lento(60), Uuid::nil(), true, tx)
            .await
            .expect_err("la 5ª viva debe rechazarse");
        assert!(matches!(err, Error::Limite(_)), "error: {err}");
        // Sin zombis: solo 4 vivas registradas.
        let vivas = e.lista().await.unwrap().into_iter().filter(|i| i.viva).count();
        assert_eq!(vivas, MAX_CONSOLAS_VIVAS);
        for id in ids {
            e.matar(&id).await.unwrap();
        }
    }

    /// [209A-1 F2] `desacoplar` de id desconocido da `NoEncontrado`.
    #[tokio::test]
    async fn desacoplar_id_desconocido_da_no_encontrado() {
        let e = EjecutorCliente::nuevo();
        let err = e
            .desacoplar("no-existe")
            .await
            .expect_err("id desconocido debe fallar");
        assert!(matches!(err, Error::NoEncontrado(_)), "error: {err}");
        assert!(e.lista().await.unwrap().is_empty());
    }

    /// [209A-1 F4] Reap por conversación: mata solo las vivas de `conv`;
    /// las de otras conversaciones siguen corriendo.
    #[tokio::test]
    async fn matar_por_conversacion_solo_mata_las_suyas() {
        let e = EjecutorCliente::nuevo();
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        for n in 0..2 {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            e.ejecutar_en_vivo(
                &format!("test-reap-a-{n}"),
                &comando_lento(60),
                conv_a,
                true,
                tx,
            )
            .await
            .unwrap();
        }
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        e.ejecutar_en_vivo("test-reap-b", &comando_lento(60), conv_b, true, tx)
            .await
            .unwrap();
        assert_eq!(e.matar_por_conversacion(conv_a).await, 2);
        // Las de A se reapean; la de B sigue viva.
        for _ in 0..20 {
            let infos = e.lista().await.unwrap();
            let vivas_a = infos.iter().filter(|i| i.viva && i.conversacion_id == conv_a).count();
            let viva_b = infos.iter().any(|i| i.viva && i.id_ejecucion == "test-reap-b");
            if vivas_a == 0 && viva_b {
                // Limpieza: no dejar la de B corriendo al salir del test.
                e.matar("test-reap-b").await.unwrap();
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        e.matar("test-reap-b").await.unwrap();
        panic!("el reap por conversación no retiró las vivas de A");
    }

    /// [209A-1 F4] Reap global: mata todas las vivas; repetir en vacío
    /// devuelve 0 sin error (idempotente, apto para el cierre de app).
    #[tokio::test]
    async fn matar_todas_vacia_las_vivas_y_en_vacio_da_cero() {
        let e = EjecutorCliente::nuevo();
        for n in 0..2 {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            e.ejecutar_en_vivo(
                &format!("test-reap-todas-{n}"),
                &comando_lento(60),
                Uuid::new_v4(),
                true,
                tx,
            )
            .await
            .unwrap();
        }
        assert_eq!(e.matar_todas().await, 2);
        for _ in 0..20 {
            let vivas = e.lista().await.unwrap().into_iter().filter(|i| i.viva).count();
            if vivas == 0 {
                assert_eq!(e.matar_todas().await, 0);
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("matar_todas no vació las vivas");
    }

    /// [119A-7 F0] Humo de la jaula: el hijo arranca con cwd = la raíz
    /// enjaulada (`en_raiz`), no con el cwd del proceso de test.
    #[tokio::test]
    async fn en_raiz_arranca_los_comandos_en_la_jaula() {
        let jaula = std::env::temp_dir().join(format!(
            "gh-jaula-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("reloj")
                .as_nanos()
        ));
        std::fs::create_dir_all(&jaula).expect("crear jaula");
        let canonica = jaula.canonicalize().expect("canonizar jaula");
        let e = EjecutorCliente::en_raiz(jaula.clone());
        // `cd` (Windows) / `pwd` (unix) reportan el cwd del hijo.
        let sonda = if cfg!(windows) { "cd" } else { "pwd" };
        let r = e.ejecutar(sonda, false).await.expect("sonda cwd");
        assert_eq!(r.codigo_salida, Some(0));
        // Windows: `canonicalize` devuelve ruta verbatim (`\\?\C:\...`)
        // mientras `cd` imprime `C:\...`; además el FS no distingue
        // mayúsculas. Se normaliza por ambos lados antes de comparar.
        let normalizar = |s: &str| {
            s.strip_prefix(r"\\?\")
                .unwrap_or(s)
                .replace('/', "\\")
                .to_lowercase()
        };
        let salida = normalizar(r.salida.trim());
        let esperada = normalizar(&canonica.to_string_lossy());
        assert!(
            salida.contains(&esperada),
            "el hijo arranca en la jaula '{esperada}', salida: {}",
            r.salida.trim()
        );
        std::fs::remove_dir_all(&jaula).ok();
    }
}

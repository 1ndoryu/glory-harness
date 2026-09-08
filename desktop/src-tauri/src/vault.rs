//! Vault de respaldos de archivos del desktop (039A-3 P3).
//!
//! Segunda alternativa a git para "volver a punto" con seguridad: cuando el
//! harness escribe un archivo (hook `RespaldoArchivos` en
//! `SandboxArchivos::escribir`), este módulo guarda una copia del contenido
//! previo dentro del workspace donde trabaja el agente, en
//! `.glory-harness/backups/` (carpeta oculta, gitignored y EXCLUIDA del
//! sandbox de escritura del agente por el core, §6.4).
//!
//! Modelo mínimo (decisión §6.6, sin SQLite):
//! - Árbol `.glory-harness/backups/<hash>/<ruta>` con el contenido previo
//!   COMPLETO en bytes (decisión §6.5: nunca el string truncado a 1MB que las
//!   tools usan para el diff — restaurar un archivo >1MB con el previo
//!   truncado lo corrompería).
//! - Índice JSONL por conversación (`.glory-harness/backups/<conv>.jsonl`):
//!   una línea por escritura `{ conversacion_id, turno_id, ruta_relativa,
//!   before_hash, after_hash, timestamp_ms, tool_name }`. Es un log; no hay
//!   consulta relacional que justifique una tabla.
//! - Dedup por `before_hash`: si el contenido ya está en el árbol, la entrada
//!   del JSONL apunta al hash existente sin re-copiar bytes.
//! - Fail-open: si el hook falla, `escribir` continúa (el respaldo es
//!   observación, no contrato del turno).
//!
//! El hook del núcleo no conoce el turno en curso: el desktop fija el
//! contexto (`conversacion_id` + `turno_id`) en el vault ANTES de cada
//! `enviar_turno` y lo limpia al terminar. Un solo turno a la vez (M1).
//!
//! Restaurar (decisión §2.3/§6.2): acción EXPLÍCITA tras un rewind, nunca
//! automática. El rewind borra turnos de la BD y devuelve sus ids; con ellos
//! se localizan en el índice las escrituras del tramo. Para cada ruta del
//! tramo se restaura el contenido PREVIO de la PRIMERA escritura del tramo
//! (el estado en el punto de rewind), solo si la fuente sigue siendo el
//! harness: se compara el hash ACTUAL contra el `after_hash` del ÚLTIMO
//! respaldo GLOBAL de esa ruta (cualquier conversación/turno). Si no
//! coincide → alguien editó fuera del harness → NO se toca y se avisa. Nunca
//! se borra un archivo (si el previo está vacío = el tramo lo creó, se omite
//! con aviso).
//!
//! GC: el rewind NO toca el vault (el tramo debe seguir restaurable). La
//! limpieza ocurre al RESTAURAR (las escrituras del tramo restaurado se
//! podan del índice y los hashes huérfanos se borran del árbol) y al
//! ELIMINAR una conversación (su índice completo + hashes huérfanos). Así el
//! disco queda acotado por acciones reales sin romper la restauración.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use glory_harness_core::error::Error as CoreError;
use glory_harness_core::sandbox::RespaldoArchivos;
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Entrada del índice JSONL (un respaldo = una escritura del harness).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct EntradaVault {
    pub conversacion_id: String,
    pub turno_id: String,
    pub ruta_relativa: String,
    pub before_hash: String,
    pub after_hash: String,
    pub timestamp_ms: i64,
    /// Tool que escribió. En v1 el hook no recibe el nombre de la tool (vive
    /// en el contexto del runtime, no en el sandbox), así que queda vacío;
    /// el campo existe para no romper el esquema cuando se pueda rellenar.
    pub tool_name: String,
}

/// Contexto del turno en curso que fija el desktop antes de `enviar_turno`.
/// Sin ids fijados (escrituras de setup), `respaldar` lo omite sin error
/// (`let-else` total en `RespaldoArchivos::respaldar`, sin `expect`).
#[derive(Default, Clone)]
pub struct ContextoTurnoVault {
    pub conversacion_id: Option<Uuid>,
    pub turno_id: Option<Uuid>,
    pub tool_name: Option<String>,
}

/// SHA-256 hex de un contenido (dedup + comprobación de fuente).
pub fn sha256_hex(datos: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(datos);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Defensa en profundidad para las rutas que salen del índice del vault:
/// solo relativas, sin `..`. (Las rutas entraron validadas por el sandbox,
/// pero el índice vive en disco y se re-validan antes de escribir/leer.)
fn relativa_valida(relativa: &str) -> bool {
    let p = Path::new(relativa);
    !p.is_absolute()
        && !p
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Implementa `RespaldoArchivos` para el desktop. Interior-mutable: el
/// contexto de turno se fija por el desktop sin reconstruir el vault.
pub struct VaultArchivos {
    workspace: PathBuf,
    raiz_backups: PathBuf,
    contexto: Mutex<ContextoTurnoVault>,
}

impl VaultArchivos {
    /// Crea el vault sobre el workspace del agente. Crea la carpeta
    /// `.glory-harness/backups` si no existe.
    pub fn nuevo(workspace: &Path) -> Self {
        let raiz_backups = workspace.join(".glory-harness").join("backups");
        let _ = fs::create_dir_all(&raiz_backups);
        Self {
            workspace: workspace.to_path_buf(),
            raiz_backups,
            contexto: Mutex::new(ContextoTurnoVault::default()),
        }
    }

    /// Fija el contexto del turno que va a escribir. Se llama ANTES de cada
    /// `enviar_turno` (M1: un turno a la vez). `None`s limpian el contexto.
    pub fn fijar_contexto(&self, ctx: ContextoTurnoVault) {
        if let Ok(mut g) = self.contexto.lock() {
            *g = ctx;
        }
    }

    fn ruta_indice(&self, conv: Uuid) -> PathBuf {
        self.raiz_backups
            .join(format!("{}.jsonl", conv.as_hyphenated()))
    }

    /// Lee las entradas del índice de una conversación (orden de append).
    pub fn leer_indice(&self, conv: Uuid) -> Vec<EntradaVault> {
        let ruta = self.ruta_indice(conv);
        let Ok(f) = fs::File::open(&ruta) else {
            return Vec::new();
        };
        let lector = std::io::BufReader::new(f);
        let mut entradas = Vec::new();
        for linea in lector.lines().map_while(Result::ok) {
            if let Ok(e) = serde_json::from_str::<EntradaVault>(&linea) {
                entradas.push(e);
            }
        }
        entradas
    }

    /// Lee TODAS las entradas de TODAS las conversaciones (solo los archivos
    /// `.jsonl` de nivel superior; los directorios `<hash>` no se recorren).
    /// Para la comprobación de fuente global por ruta.
    fn leer_indices_globales(&self) -> Vec<EntradaVault> {
        let Ok(lecturas) = fs::read_dir(&self.raiz_backups) else {
            return Vec::new();
        };
        let mut todas = Vec::new();
        for entrada in lecturas.flatten() {
            let ruta = entrada.path();
            if ruta.is_file() && ruta.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let Ok(f) = fs::File::open(&ruta) else {
                    continue;
                };
                let lector = std::io::BufReader::new(f);
                for linea in lector.lines().map_while(Result::ok) {
                    if let Ok(e) = serde_json::from_str::<EntradaVault>(&linea) {
                        todas.push(e);
                    }
                }
            }
        }
        todas
    }

    /// Reescribe el índice de una conversación con las entradas indicadas.
    fn escribir_indice(&self, conv: Uuid, entradas: &[EntradaVault]) {
        let ruta = self.ruta_indice(conv);
        let Ok(f) = fs::File::create(&ruta) else {
            return;
        };
        let mut w = std::io::BufWriter::new(f);
        for e in entradas {
            if let Ok(linea) = serde_json::to_string(e) {
                let _ = writeln!(w, "{linea}");
            }
        }
    }

    /// Borra los directorios `<hash>` que ya no referencia NINGUNA entrada de
    /// ningún índice (GC de disco). Best-effort.
    fn gc_hashes_huerfanos(&self) {
        let referenciados: HashSet<String> = self
            .leer_indices_globales()
            .into_iter()
            .map(|e| e.before_hash)
            .collect();
        let Ok(lecturas) = fs::read_dir(&self.raiz_backups) else {
            return;
        };
        for entrada in lecturas.flatten() {
            let ruta = entrada.path();
            let nombre_hash = ruta.file_name().map(|n| n.to_string_lossy().into_owned());
            let es_dir_hash = ruta.is_dir()
                && nombre_hash
                    .as_ref()
                    .map(|n| n.len() == 64 && n.chars().all(|c| c.is_ascii_hexdigit()))
                    .unwrap_or(false);
            if es_dir_hash
                && nombre_hash
                    .as_ref()
                    .map(|n| !referenciados.contains(n))
                    .unwrap_or(false)
            {
                let _ = fs::remove_dir_all(&ruta);
            }
        }
    }

    /// Núcleo del hook: guarda el previo (dedup por `before_hash`) y anota la
    /// entrada en el índice de la conversación. Error → el llamador lo trata
    /// como fail-open.
    pub fn registrar_escritura(
        &self,
        conv: Uuid,
        turno: Uuid,
        relativa: &str,
        previo_bytes: &[u8],
        nuevo_contenido: &str,
        tool_name: Option<&str>,
    ) -> Result<(), CoreError> {
        let before_hash = sha256_hex(previo_bytes);
        let after_hash = sha256_hex(nuevo_contenido.as_bytes());
        // Dedup: si el contenido previo ya está en el árbol, no re-copiar.
        let ruta_hash = self.raiz_backups.join(&before_hash);
        if !ruta_hash.exists() {
            let destino = ruta_hash.join(relativa.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(padre) = destino.parent() {
                fs::create_dir_all(padre)
                    .map_err(|e| CoreError::Persistencia(format!("vault: crear dir: {e}")))?;
            }
            fs::write(&destino, previo_bytes)
                .map_err(|e| CoreError::Persistencia(format!("vault: escribir previo: {e}")))?;
        }
        // Índice JSONL por conversación (append).
        let entrada = EntradaVault {
            conversacion_id: conv.as_hyphenated().to_string(),
            turno_id: turno.as_hyphenated().to_string(),
            ruta_relativa: relativa.to_string(),
            before_hash,
            after_hash,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            tool_name: tool_name.unwrap_or("").to_string(),
        };
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.ruta_indice(conv))
            .map_err(|e| CoreError::Persistencia(format!("vault: índice: {e}")))?;
        let linea = serde_json::to_string(&entrada)
            .map_err(|e| CoreError::Persistencia(format!("vault: serializar: {e}")))?;
        writeln!(f, "{linea}")
            .map_err(|e| CoreError::Persistencia(format!("vault: escribir índice: {e}")))?;
        Ok(())
    }

    /// Contenido actual de una ruta del workspace (`None` si no existe).
    pub fn contenido_actual(&self, ruta_relativa: &str) -> Option<Vec<u8>> {
        if !relativa_valida(ruta_relativa) {
            return None;
        }
        fs::read(
            self.workspace
                .join(ruta_relativa.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .ok()
    }

    /// Contenido respaldado de un hash (bytes). `None` si falta.
    fn contenido_del_hash(&self, hash: &str, relativa: &str) -> Option<Vec<u8>> {
        if !relativa_valida(relativa) {
            return None;
        }
        fs::read(
            self.raiz_backups
                .join(hash)
                .join(relativa.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .ok()
    }

    /// Último respaldo GLOBAL de una ruta (cualquier conversación/turno).
    /// "Último" = mayor `timestamp_ms`; a igualdad, cualquiera (los `after`
    /// de un mismo estado son idénticos).
    fn ultimo_respaldo_global(&self, relativa: &str) -> Option<EntradaVault> {
        self.leer_indices_globales()
            .into_iter()
            .filter(|e| e.ruta_relativa == relativa)
            .max_by_key(|e| e.timestamp_ms)
    }

    /// Primeras escrituras del tramo por ruta (los turnos borrados por el
    /// rewind). Cada entrada devuelta es la PRIMERA escritura de ese turno
    /// sobre la ruta: su `before_hash` es el contenido en el punto de rewind
    /// (objetivo de la restauración).
    pub fn archivos_del_tramo(&self, turnos_tramo: &[Uuid]) -> Vec<EntradaVault> {
        let globales = self.leer_indices_globales();
        let set_tramo: HashSet<String> = turnos_tramo
            .iter()
            .map(|t| t.as_hyphenated().to_string())
            .collect();
        let mut por_ruta: HashMap<String, EntradaVault> = HashMap::new();
        for e in &globales {
            if set_tramo.contains(&e.turno_id) {
                por_ruta
                    .entry(e.ruta_relativa.clone())
                    .or_insert_with(|| e.clone());
            }
        }
        por_ruta.into_values().collect()
    }

    /// Restaura una ruta al estado del punto de rewind SOLO si la fuente
    /// sigue siendo el harness (decisión §6.2). Nunca borra archivos.
    ///
    /// `punto` = primera escritura del tramo para esa ruta (su `before_hash`
    /// es el contenido en el punto de rewind). Comprobación: hash ACTUAL ==
    /// `after_hash` del ÚLTIMO respaldo GLOBAL de la ruta → el harness fue la
    /// última fuente → restaurar el `before` del punto. Si no coincide →
    /// edición externa → NO tocar.
    pub fn restaurar_ruta(&self, punto: &EntradaVault) -> Result<RestauracionArchivo, String> {
        let ruta_rel = &punto.ruta_relativa;
        if !relativa_valida(ruta_rel) {
            return Ok(RestauracionArchivo {
                ruta: ruta_rel.clone(),
                estado: "omitido".into(),
                detalle: Some("ruta inválida en el índice del vault".into()),
            });
        }
        let actual = self.contenido_actual(ruta_rel).unwrap_or_default();
        let hash_actual = sha256_hex(&actual);
        let fuente_harness = match self.ultimo_respaldo_global(ruta_rel) {
            Some(u) => u.after_hash == hash_actual,
            None => false, // sin historial global: no hay fuente que verificar
        };
        if !fuente_harness {
            return Ok(RestauracionArchivo {
                ruta: ruta_rel.clone(),
                estado: "cambio_externo".into(),
                detalle: Some(
                    "el archivo cambió fuera del harness desde el turno; no se toca".into(),
                ),
            });
        }
        let previo = match self.contenido_del_hash(&punto.before_hash, ruta_rel) {
            Some(bytes) => bytes,
            None => {
                return Ok(RestauracionArchivo {
                    ruta: ruta_rel.clone(),
                    estado: "omitido".into(),
                    detalle: Some("respaldo del punto no encontrado en el vault".into()),
                });
            }
        };
        // Nunca borrar: un previo vacío significa que el archivo no existía en
        // el punto (el tramo lo creó). Restaurar "a no existir" sería borrarlo
        // → se omite con aviso (v1 no borra archivos).
        if previo.is_empty() {
            return Ok(RestauracionArchivo {
                ruta: ruta_rel.clone(),
                estado: "omitido".into(),
                detalle: Some(
                    "el archivo no existía en ese punto (se creó después); no se borra".into(),
                ),
            });
        }
        let ruta_abs = self
            .workspace
            .join(ruta_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(padre) = ruta_abs.parent() {
            fs::create_dir_all(padre)
                .map_err(|e| format!("no se pudo crear el directorio: {e}"))?;
        }
        fs::write(&ruta_abs, &previo)
            .map_err(|e| format!("no se pudo escribir el archivo restaurado: {e}"))?;
        Ok(RestauracionArchivo {
            ruta: ruta_rel.clone(),
            estado: "restaurado".into(),
            detalle: None,
        })
    }

    /// Poda del índice de una conversación las entradas de los turnos del
    /// tramo que corresponden a rutas restauradas, y GC de hashes huérfanos.
    /// Se llama tras una restauración satisfactoria: las escrituras del tramo
    /// ya se deshicieron, su registro es historia muerta. Best-effort.
    fn purgar_tramo_restaurado(&self, conv: Uuid, turnos: &[Uuid], rutas_restauradas: &[String]) {
        let set_turnos: HashSet<String> = turnos
            .iter()
            .map(|t| t.as_hyphenated().to_string())
            .collect();
        let set_rutas: HashSet<&str> = rutas_restauradas.iter().map(|s| s.as_str()).collect();
        let conservar: Vec<EntradaVault> = self
            .leer_indice(conv)
            .into_iter()
            .filter(|e| {
                !(set_turnos.contains(&e.turno_id) && set_rutas.contains(e.ruta_relativa.as_str()))
            })
            .collect();
        self.escribir_indice(conv, &conservar);
        self.gc_hashes_huerfanos();
    }

    /// Elimina el índice completo de una conversación (al borrarla) + GC.
    pub fn eliminar_conversacion(&self, conv: Uuid) {
        let _ = fs::remove_file(self.ruta_indice(conv));
        self.gc_hashes_huerfanos();
    }

    /// Operación "restaurar archivos de este tramo" completa: para cada ruta
    /// tocada por los turnos borrados intenta restaurar al punto de rewind.
    /// Devuelve el detalle por archivo. Las rutas restauradas se purgan.
    pub fn restaurar_tramo(&self, turnos: &[Uuid]) -> ResultadoRestauracion {
        let mut resultado = ResultadoRestauracion::default();
        let puntos = self.archivos_del_tramo(turnos);
        if puntos.is_empty() {
            return resultado;
        }
        // Todos los turnos del tramo son de la misma conversación (la del
        // rewind): se toma la primera entrada para saber qué índice podar.
        let conv = puntos[0]
            .conversacion_id
            .parse::<Uuid>()
            .unwrap_or(Uuid::nil());
        let mut rutas_restauradas: Vec<String> = Vec::new();
        for punto in &puntos {
            match self.restaurar_ruta(punto) {
                Ok(r) => {
                    if r.estado == "restaurado" {
                        rutas_restauradas.push(r.ruta.clone());
                        resultado.restaurados.push(r);
                    } else {
                        resultado.omitidos.push(r);
                    }
                }
                Err(e) => {
                    resultado.omitidos.push(RestauracionArchivo {
                        ruta: punto.ruta_relativa.clone(),
                        estado: "error".into(),
                        detalle: Some(e),
                    });
                }
            }
        }
        if !rutas_restauradas.is_empty() && conv != Uuid::nil() {
            self.purgar_tramo_restaurado(conv, turnos, &rutas_restauradas);
        }
        resultado
    }
}

impl RespaldoArchivos for VaultArchivos {
    fn respaldar(
        &self,
        relativa: &str,
        previo_bytes: &[u8],
        nuevo_contenido: &str,
    ) -> Result<(), CoreError> {
        let ctx = match self.contexto.lock() {
            Ok(g) => g.clone(),
            Err(_) => return Ok(()), // sin contexto fiable: no respaldar
        };
        // Sin conversación/turno fijados (p. ej. escrituras de setup): el
        // respaldo no se puede atribuir a un tramo → se omite sin error.
        // `let-else` total en vez de `expect`: nunca pánico en producción.
        let (Some(conv), Some(turno)) = (ctx.conversacion_id, ctx.turno_id) else {
            return Ok(());
        };
        self.registrar_escritura(
            conv,
            turno,
            relativa,
            previo_bytes,
            nuevo_contenido,
            ctx.tool_name.as_deref(),
        )
    }
}

/// Resultado de restaurar un archivo: qué se hizo (o por qué no).
#[derive(serde::Serialize, Clone, PartialEq, Debug)]
pub struct RestauracionArchivo {
    pub ruta: String,
    /// "restaurado" | "cambio_externo" | "omitido" | "error"
    pub estado: String,
    pub detalle: Option<String>,
}

/// Resultado de la operación "restaurar archivos de este tramo".
#[derive(serde::Serialize, Clone, Default, Debug)]
pub struct ResultadoRestauracion {
    pub restaurados: Vec<RestauracionArchivo>,
    pub omitidos: Vec<RestauracionArchivo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use glory_harness_core::sandbox::SandboxArchivos;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    static CONTADOR_TMP: AtomicU64 = AtomicU64::new(0);

    fn dir_tmp() -> PathBuf {
        let n = CONTADOR_TMP.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join("glory-harness-vault-test")
            .join(format!("{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Sandbox del core con el vault cableado como hook (como hace el desktop
    /// tras construir el runtime).
    fn sandbox_con_vault(dir: &Path) -> (Arc<SandboxArchivos>, Arc<VaultArchivos>) {
        let sandbox = Arc::new(SandboxArchivos::nuevo(dir).unwrap());
        let vault = Arc::new(VaultArchivos::nuevo(dir));
        sandbox.con_respaldo(Some(Arc::clone(&vault) as Arc<dyn RespaldoArchivos>));
        (sandbox, vault)
    }

    fn ctx(conv: Uuid, turno: Uuid) -> ContextoTurnoVault {
        ContextoTurnoVault {
            conversacion_id: Some(conv),
            turno_id: Some(turno),
            tool_name: None,
        }
    }

    /// Fixture 1: dos turnos tocan el mismo archivo → rewind al 1º +
    /// restaurar restaura correctamente (sin falso "cambió fuera").
    #[test]
    fn vault_fixture1_rewind_primer_turno_y_restaura() {
        let dir = dir_tmp();
        let (sandbox, vault) = sandbox_con_vault(&dir);
        fs::write(dir.join("a.txt"), "base").unwrap();
        let conv = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();
        // T1 escribe base→uno; T2 escribe uno→dos.
        vault.fijar_contexto(ctx(conv, t1));
        sandbox.escribir("a.txt", "uno").unwrap();
        vault.fijar_contexto(ctx(conv, t2));
        sandbox.escribir("a.txt", "dos").unwrap();
        // Rewind al 1º borra T1 y T2; restaurar debe volver al estado "base".
        let turnos = vec![t1, t2];
        let puntos = vault.archivos_del_tramo(&turnos);
        assert_eq!(puntos.len(), 1);
        let r = vault.restaurar_tramo(&turnos);
        assert_eq!(r.restaurados.len(), 1, "esperado 1 restaurado: {:?}", r);
        assert_eq!(r.omitidos.len(), 0);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "base");
        // Tras restaurar, el tramo se purgó: una nueva restauración es no-op.
        let r2 = vault.restaurar_tramo(&turnos);
        assert!(r2.restaurados.is_empty() && r2.omitidos.is_empty());
    }

    /// Fixture 2: edición manual externa entre el turno y la restauración →
    /// avisa y NO toca.
    #[test]
    fn vault_fixture2_cambio_externo_no_toca() {
        let dir = dir_tmp();
        let (sandbox, vault) = sandbox_con_vault(&dir);
        fs::write(dir.join("a.txt"), "base").unwrap();
        let conv = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        vault.fijar_contexto(ctx(conv, t1));
        sandbox.escribir("a.txt", "uno").unwrap();
        // Edición externa a mano (fuera del harness).
        fs::write(dir.join("a.txt"), "editado a mano").unwrap();
        let turnos = vec![t1];
        let r = vault.restaurar_tramo(&turnos);
        assert!(r.restaurados.is_empty());
        assert_eq!(r.omitidos.len(), 1);
        assert_eq!(r.omitidos[0].estado, "cambio_externo");
        // No se tocó el archivo.
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "editado a mano"
        );
    }

    /// Fixture 3: archivo >1MB (el previo se guarda y restaura COMPLETO, sin
    /// truncar al límite de 1MB que las tools usan para el diff).
    #[test]
    fn vault_fixture3_previo_mas_de_un_megabyte_completo() {
        let dir = dir_tmp();
        let (sandbox, vault) = sandbox_con_vault(&dir);
        // 1.5 MB de contenido original.
        let grande = "x".repeat(1_500_000);
        fs::write(dir.join("grande.txt"), &grande).unwrap();
        let conv = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        vault.fijar_contexto(ctx(conv, t1));
        sandbox.escribir("grande.txt", "pequeño").unwrap();
        let turnos = vec![t1];
        let r = vault.restaurar_tramo(&turnos);
        assert_eq!(r.restaurados.len(), 1, "esperado 1 restaurado: {:?}", r);
        let bytes = fs::read(dir.join("grande.txt")).unwrap();
        assert_eq!(
            bytes.len(),
            1_500_000,
            "el previo debe restaurarse COMPLETO"
        );
        assert_eq!(bytes, grande.as_bytes());
    }

    /// El hook sin contexto fijado (p. ej. escrituras de setup) no respalda y
    /// no rompe la escritura.
    #[test]
    fn vault_sin_contexto_no_respalda_pero_escribe() {
        let dir = dir_tmp();
        let (sandbox, _vault) = sandbox_con_vault(&dir);
        fs::write(dir.join("b.txt"), "base").unwrap();
        sandbox.escribir("b.txt", "nuevo").unwrap();
        assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "nuevo");
        // El índice de esa conversación no existe (no hubo escritura atribuible).
        assert!(vault_leer_indices(&dir).is_empty());
    }

    fn vault_leer_indices(dir: &Path) -> Vec<EntradaVault> {
        let raiz = dir.join(".glory-harness").join("backups");
        let mut todas = Vec::new();
        if let Ok(lecturas) = fs::read_dir(&raiz) {
            for e in lecturas.flatten() {
                let ruta = e.path();
                if ruta.is_file() && ruta.extension().map(|x| x == "jsonl").unwrap_or(false) {
                    if let Ok(contenido) = fs::read_to_string(&ruta) {
                        for linea in contenido.lines() {
                            if let Ok(ent) = serde_json::from_str::<EntradaVault>(linea) {
                                todas.push(ent);
                            }
                        }
                    }
                }
            }
        }
        todas
    }
}

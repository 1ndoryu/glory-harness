//! Dominio de metas por conversación (109A-5 F1).
//!
//! Este módulo contiene únicamente la máquina de estados y el reloj. No conoce
//! Tauri, HTTP ni SQLite: los transportes deben persistir el `EstadoMeta`
//! resultante de forma atómica y convertir sus errores en respuestas explícitas.
//! El logro es declarado, no una verificación externa de que el mundo haya
//! alcanzado la meta.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::MetaConversacionPersistida;

/// Límite compartido para metas y entradas del historial persistido.
pub const MAX_META_CHARS: usize = 8_000;
/// El historial conserva solo los logros más recientes.
pub const MAX_LOGROS_META: usize = 20;

/// [109A-5 F4] Turnos consecutivos con el MISMO motivo de bloqueo antes de que
/// el backend pause la meta y avise al usuario.
///
/// Tres es el umbral del plan y no una cifra redonda: al primero el bloqueo
/// puede ser un tropiezo normal, al segundo el usuario ya tuvo una oportunidad
/// de desatascarlo, y al tercero callarse sería dejar la meta corriendo un reloj
/// que no avanza. Quien pausa es el backend, nunca el agente.
pub const UMBRAL_BLOQUEO_TURNOS: u32 = 3;

/// Acción de ciclo de vida solicitada sobre la meta de una conversación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComandoMeta {
    /// Crea una meta o edita la activa conservando su reloj y pausa.
    Fijar { texto: String },
    /// Elimina la meta activa; el historial de logros se conserva.
    Limpiar,
    /// Congela el reloj en el instante indicado.
    Pausar,
    /// Reanuda el reloj excluyendo el intervalo pausado.
    Reanudar,
    /// Registra el logro y limpia la meta activa.
    Lograr { turno_id: Uuid },
}

/// Meta actualmente fijada en una conversación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaActiva {
    pub texto: String,
    pub iniciada_en: DateTime<Utc>,
    pub pausada_en: Option<DateTime<Utc>>,
}

/// Evidencia declarada de una meta lograda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogroMeta {
    pub meta: String,
    pub lograda_en: DateTime<Utc>,
    pub elapsed_ms: u64,
    pub turno_id: Uuid,
}

/// Estado completo persistible de la meta de una conversación.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstadoMeta {
    pub activa: Option<MetaActiva>,
    pub logros: Vec<LogroMeta>,
}

impl EstadoMeta {
    /// Texto de la meta vigente, si la hay (`None` = sin meta, solo historial).
    pub fn texto_activo(&self) -> Option<&str> {
        self.activa.as_ref().map(|activa| activa.texto.as_str())
    }

    /// Convierte la representación SQL en dominio y falla cerrado ante
    /// timestamps o JSON corruptos. El historial legado se acota al leer.
    pub fn desde_persistida(valor: MetaConversacionPersistida) -> Result<Self, ErrorMeta> {
        let tiene_texto = valor.texto.is_some();
        let activa = match valor.texto {
            Some(texto) => {
                let inicio = valor.iniciada_en.ok_or_else(|| {
                    ErrorMeta::EstadoPersistidoInvalido("meta sin fecha de inicio".into())
                })?;
                Some(MetaActiva {
                    texto: normalizar_texto(&texto)?,
                    iniciada_en: parsear_fecha(&inicio)?,
                    pausada_en: valor
                        .pausada_en
                        .as_deref()
                        .map(parsear_fecha)
                        .transpose()?,
                })
            }
            None => None,
        };
        if activa.as_ref().is_some_and(|a| a.pausada_en.is_some()) && !tiene_texto {
            return Err(ErrorMeta::EstadoPersistidoInvalido(
                "pausa sin meta activa".into(),
            ));
        }
        let mut logros: Vec<LogroMeta> = serde_json::from_str(&valor.logros_json).map_err(|e| {
            ErrorMeta::EstadoPersistidoInvalido(format!("historial JSON: {e}"))
        })?;
        if logros.len() > MAX_LOGROS_META {
            let exceso = logros.len() - MAX_LOGROS_META;
            logros.drain(..exceso);
        }
        Ok(Self { activa, logros })
    }

    /// Convierte el dominio a la fila que SQLite guarda. El historial siempre
    /// queda limitado antes de serializar, incluso si vino de una BD antigua.
    pub fn a_persistida(&self) -> Result<MetaConversacionPersistida, ErrorMeta> {
        let (texto, iniciada_en, pausada_en) = match &self.activa {
            Some(activa) => (
                Some(activa.texto.clone()),
                Some(activa.iniciada_en.to_rfc3339()),
                activa.pausada_en.map(|fecha| fecha.to_rfc3339()),
            ),
            None => (None, None, None),
        };
        let inicio = self.logros.len().saturating_sub(MAX_LOGROS_META);
        let logros = &self.logros[inicio..];
        let logros_json = serde_json::to_string(logros)
            .map_err(|e| ErrorMeta::EstadoPersistidoInvalido(format!("historial JSON: {e}")))?;
        Ok(MetaConversacionPersistida {
            texto,
            iniciada_en,
            pausada_en,
            logros_json,
        })
    }
}

/// Resultado de una transición, incluyendo el logro recién creado cuando aplica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultadoMeta {
    pub estado: EstadoMeta,
    pub logro: Option<LogroMeta>,
}

/// [109A-5 F4] Veredicto de escalar el bloqueo declarado en el plan al cerrar
/// un turno. Solo `Pausada` cambia el reloj: los demás casos existen para que el
/// transporte pueda registrar por qué NO se pausó sin adivinar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "estado", rename_all = "snake_case")]
pub enum ResultadoBloqueo {
    /// No hay bloqueo vigente o no hay meta activa (nada que congelar).
    Inaplicable,
    /// Bloqueo contado, todavía por debajo del umbral.
    Contado { motivo: String, turnos: u32 },
    /// La meta ya estaba pausada: la escalada no repite el aviso.
    YaPausada { motivo: String, turnos: u32 },
    /// La meta se pausó AHORA por este bloqueo (transición real y persistida).
    Pausada { motivo: String, turnos: u32 },
}

/// Errores de dominio: ningún comando inválido muta el estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorMeta {
    TextoVacio,
    TextoDemasiadoLargo,
    SinMetaActiva,
    YaPausada,
    NoPausada,
    FechaAnteriorAlInicio,
    FechaAnteriorALaPausa,
    /// La conversación indicada no existe o no pertenece al usuario.
    ConversacionInexistente,
    /// El comando exige conversación (pausar/reanudar/lograr) pero el panel
    /// sigue en borrador, sin fila donde anclar el reloj.
    SinConversacion,
    /// Payload del panel malformado (acción desconocida, `meta` inesperada,
    /// `turno_id` malformado).
    ComandoInvalido(String),
    /// `lograr` sin el turno que lo respalda.
    TurnoRequerido,
    Persistencia(String),
    EstadoPersistidoInvalido(String),
}

impl ErrorMeta {
    /// Código estable para transportes: el modo web lo usa como `code` de la
    /// respuesta; Tauri y la CLI solo muestran el `Display`.
    pub fn codigo(&self) -> &'static str {
        match self {
            Self::TextoVacio => "meta_vacia",
            Self::TextoDemasiadoLargo => "meta_larga",
            Self::SinMetaActiva => "sin_meta_activa",
            Self::YaPausada => "meta_ya_pausada",
            Self::NoPausada => "meta_no_pausada",
            Self::FechaAnteriorAlInicio | Self::FechaAnteriorALaPausa => "reloj_meta_invalido",
            Self::ConversacionInexistente => "no_encontrado",
            Self::SinConversacion => "sin_conversacion",
            Self::ComandoInvalido(_) => "peticion_invalida",
            Self::TurnoRequerido => "turno_requerido",
            Self::Persistencia(_) => "persistencia",
            Self::EstadoPersistidoInvalido(_) => "meta_invalida",
        }
    }
}

impl std::fmt::Display for ErrorMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mensaje = match self {
            Self::TextoVacio => "la meta no puede estar vacía".to_string(),
            Self::TextoDemasiadoLargo => {
                format!("la meta supera el límite de {MAX_META_CHARS} caracteres")
            }
            Self::SinMetaActiva => "no hay una meta activa".to_string(),
            Self::YaPausada => "la meta ya está pausada".to_string(),
            Self::NoPausada => "la meta no está pausada".to_string(),
            Self::FechaAnteriorAlInicio => "la fecha está antes del inicio de la meta".to_string(),
            Self::FechaAnteriorALaPausa => {
                "la fecha está antes del inicio de la pausa".to_string()
            }
            Self::ConversacionInexistente => {
                "la conversación de esta sesión ya no existe".to_string()
            }
            Self::SinConversacion => {
                "la conversación aún no existe: envía el primer mensaje".to_string()
            }
            Self::ComandoInvalido(motivo) => return write!(f, "comando de meta inválido: {motivo}"),
            Self::TurnoRequerido => "`lograr` requiere el `turno_id` que respalda el logro".to_string(),
            Self::Persistencia(motivo) => return write!(f, "error de persistencia de meta: {motivo}"),
            Self::EstadoPersistidoInvalido(motivo) => {
                return write!(f, "estado de meta inválido: {motivo}")
            }
        };
        f.write_str(&mensaje)
    }
}

impl std::error::Error for ErrorMeta {}

/// Traduce el payload del panel (`meta`/`accion`/`turno_id`) al comando de
/// dominio. Sin `accion` se conserva el contrato anterior del panel: texto
/// presente ⇒ fijar, ausente o vacío ⇒ limpiar.
pub fn comando_desde_payload(
    meta: Option<String>,
    accion: Option<&str>,
    turno_id: Option<&str>,
) -> Result<ComandoMeta, ErrorMeta> {
    let Some(accion) = accion.map(str::trim).filter(|valor| !valor.is_empty()) else {
        return Ok(match meta {
            Some(texto) if !texto.trim().is_empty() => ComandoMeta::Fijar { texto },
            _ => ComandoMeta::Limpiar,
        });
    };
    if accion != "fijar" && meta.is_some() {
        return Err(ErrorMeta::ComandoInvalido(format!(
            "`{accion}` no acepta `meta`"
        )));
    }
    match accion {
        // Vacío explícito NO se degrada a limpiar: se rechaza con el mismo
        // error que cualquier meta vacía, para no confundir dos intenciones.
        "fijar" => Ok(ComandoMeta::Fijar {
            texto: meta.unwrap_or_default(),
        }),
        "limpiar" => Ok(ComandoMeta::Limpiar),
        "pausar" => Ok(ComandoMeta::Pausar),
        "reanudar" => Ok(ComandoMeta::Reanudar),
        "lograr" => {
            let turno = turno_id
                .map(str::trim)
                .filter(|valor| !valor.is_empty())
                .ok_or(ErrorMeta::TurnoRequerido)?;
            let turno_id = Uuid::parse_str(turno)
                .map_err(|_| ErrorMeta::ComandoInvalido("`turno_id` malformado".into()))?;
            Ok(ComandoMeta::Lograr { turno_id })
        }
        otra => Err(ErrorMeta::ComandoInvalido(format!(
            "acción de meta desconocida: `{otra}`"
        ))),
    }
}

/// Aplica un comando a la meta EN MEMORIA de un borrador (panel sin
/// conversación todavía): solo fijar y limpiar tienen sentido, porque pausar,
/// reanudar y lograr necesitan el reloj durable de una conversación. El valor
/// previo no se consulta (`fijar` sustituye y `limpiar` vacía), así que no se
/// recibe: el llamador asigna el resultado o lo descarta.
pub fn aplicar_en_borrador(comando: ComandoMeta) -> Result<Option<String>, ErrorMeta> {
    match comando {
        ComandoMeta::Fijar { texto } => Ok(Some(normalizar_texto(&texto)?)),
        ComandoMeta::Limpiar => Ok(None),
        ComandoMeta::Pausar | ComandoMeta::Reanudar | ComandoMeta::Lograr { .. } => {
            Err(ErrorMeta::SinConversacion)
        }
    }
}

/// Resuelve una transición sin efectos secundarios. El llamador debe guardar
/// el `estado` devuelto solo después de que la operación haya terminado bien.
pub fn resolver_meta(
    comando: ComandoMeta,
    estado: &EstadoMeta,
    ahora: DateTime<Utc>,
) -> Result<ResultadoMeta, ErrorMeta> {
    let mut siguiente = estado.clone();
    let mut logro = None;
    match comando {
        ComandoMeta::Fijar { texto } => {
            let texto = normalizar_texto(&texto)?;
            if let Some(activa) = siguiente.activa.as_mut() {
                activa.texto = texto;
            } else {
                siguiente.activa = Some(MetaActiva {
                    texto,
                    iniciada_en: ahora,
                    pausada_en: None,
                });
            }
        }
        ComandoMeta::Limpiar => siguiente.activa = None,
        ComandoMeta::Pausar => {
            let activa = siguiente.activa.as_mut().ok_or(ErrorMeta::SinMetaActiva)?;
            if activa.pausada_en.is_some() {
                return Err(ErrorMeta::YaPausada);
            }
            validar_no_anterior(ahora, activa.iniciada_en)?;
            activa.pausada_en = Some(ahora);
        }
        ComandoMeta::Reanudar => {
            let activa = siguiente.activa.as_mut().ok_or(ErrorMeta::SinMetaActiva)?;
            let pausada_en = activa.pausada_en.ok_or(ErrorMeta::NoPausada)?;
            validar_no_anterior(ahora, pausada_en)?;
            let elapsed = duracion_ms(pausada_en - activa.iniciada_en)?;
            activa.iniciada_en = ahora - Duration::milliseconds(elapsed as i64);
            activa.pausada_en = None;
        }
        ComandoMeta::Lograr { turno_id } => {
            let activa = siguiente.activa.take().ok_or(ErrorMeta::SinMetaActiva)?;
            let fin_reloj = activa.pausada_en.unwrap_or(ahora);
            validar_no_anterior(fin_reloj, activa.iniciada_en)?;
            if activa.pausada_en.is_some() {
                validar_no_anterior(ahora, fin_reloj)?;
            }
            let nuevo = LogroMeta {
                meta: activa.texto,
                lograda_en: ahora,
                elapsed_ms: duracion_ms(fin_reloj - activa.iniciada_en)?,
                turno_id,
            };
            siguiente.logros.push(nuevo.clone());
            if siguiente.logros.len() > MAX_LOGROS_META {
                let exceso = siguiente.logros.len() - MAX_LOGROS_META;
                siguiente.logros.drain(..exceso);
            }
            logro = Some(nuevo);
        }
    }
    Ok(ResultadoMeta { estado: siguiente, logro })
}

/// Calcula el tiempo neto de una meta activa sin mutarla.
pub fn elapsed_ms(estado: &MetaActiva, ahora: DateTime<Utc>) -> Result<u64, ErrorMeta> {
    let fin = estado.pausada_en.unwrap_or(ahora);
    validar_no_anterior(fin, estado.iniciada_en)?;
    if estado.pausada_en.is_some() {
        validar_no_anterior(ahora, fin)?;
    }
    duracion_ms(fin - estado.iniciada_en)
}

fn parsear_fecha(valor: &str) -> Result<DateTime<Utc>, ErrorMeta> {
    DateTime::parse_from_rfc3339(valor)
        .map(|fecha| fecha.with_timezone(&Utc))
        .map_err(|e| ErrorMeta::EstadoPersistidoInvalido(format!("timestamp: {e}")))
}

fn normalizar_texto(texto: &str) -> Result<String, ErrorMeta> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Err(ErrorMeta::TextoVacio);
    }
    if texto.chars().count() > MAX_META_CHARS {
        return Err(ErrorMeta::TextoDemasiadoLargo);
    }
    Ok(texto.to_owned())
}

fn validar_no_anterior(fecha: DateTime<Utc>, referencia: DateTime<Utc>) -> Result<(), ErrorMeta> {
    if fecha < referencia {
        Err(ErrorMeta::FechaAnteriorAlInicio)
    } else {
        Ok(())
    }
}

fn duracion_ms(duracion: Duration) -> Result<u64, ErrorMeta> {
    u64::try_from(duracion.num_milliseconds()).map_err(|_| ErrorMeta::FechaAnteriorAlInicio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ahora() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0)
            .single()
            .expect("fecha fija válida")
    }

    #[test]
    fn fija_y_edita_sin_reiniciar_reloj() {
        let estado = EstadoMeta::default();
        let inicio = ahora();
        let fijada = resolver_meta(
            ComandoMeta::Fijar { texto: "  primera  ".into() },
            &estado,
            inicio,
        )
        .expect("fija");
        let editada = resolver_meta(
            ComandoMeta::Fijar { texto: "segunda".into() },
            &fijada.estado,
            inicio + Duration::seconds(4),
        )
        .expect("edita");
        let activa = editada.estado.activa.expect("activa");
        assert_eq!(activa.texto, "segunda");
        assert_eq!(activa.iniciada_en, inicio);
    }

    #[test]
    fn pausa_reanuda_y_mide_tiempo_neto() {
        let inicio = ahora();
        let fijada = resolver_meta(
            ComandoMeta::Fijar { texto: "meta".into() },
            &EstadoMeta::default(),
            inicio,
        )
        .expect("fija");
        let pausada = resolver_meta(
            ComandoMeta::Pausar,
            &fijada.estado,
            inicio + Duration::seconds(10),
        )
        .expect("pausa");
        assert_eq!(elapsed_ms(pausada.estado.activa.as_ref().expect("activa"), inicio + Duration::seconds(100)).expect("reloj"), 10_000);
        let reanudada = resolver_meta(
            ComandoMeta::Reanudar,
            &pausada.estado,
            inicio + Duration::seconds(100),
        )
        .expect("reanuda");
        assert_eq!(elapsed_ms(reanudada.estado.activa.as_ref().expect("activa"), inicio + Duration::seconds(105)).expect("reloj"), 15_000);
    }

    #[test]
    fn logra_ancla_turno_y_conserva_historial_limitado() {
        let inicio = ahora();
        let fijada = resolver_meta(
            ComandoMeta::Fijar { texto: "meta".into() },
            &EstadoMeta::default(),
            inicio,
        )
        .expect("fija");
        let turno = Uuid::new_v4();
        let logrado = resolver_meta(
            ComandoMeta::Lograr { turno_id: turno },
            &fijada.estado,
            inicio + Duration::seconds(77),
        )
        .expect("logra");
        assert!(logrado.estado.activa.is_none());
        let logro = logrado.logro.expect("logro");
        assert_eq!(logro.elapsed_ms, 77_000);
        assert_eq!(logro.turno_id, turno);
        assert_eq!(logrado.estado.logros.len(), 1);

        let mut estado = logrado.estado;
        for i in 0..(MAX_LOGROS_META + 2) {
            let fijada = resolver_meta(
                ComandoMeta::Fijar { texto: format!("meta {i}") },
                &estado,
                inicio + Duration::seconds(i as i64 + 100),
            )
            .expect("fija siguiente");
            estado = resolver_meta(
                ComandoMeta::Lograr { turno_id: Uuid::new_v4() },
                &fijada.estado,
                inicio + Duration::seconds(i as i64 + 101),
            )
            .expect("logra siguiente")
            .estado;
        }
        assert_eq!(estado.logros.len(), MAX_LOGROS_META);
    }

    #[test]
    fn rechaza_transiciones_invalidas_sin_mutar() {
        let estado = EstadoMeta::default();
        assert_eq!(resolver_meta(ComandoMeta::Pausar, &estado, ahora()).unwrap_err(), ErrorMeta::SinMetaActiva);
        assert_eq!(resolver_meta(ComandoMeta::Reanudar, &estado, ahora()).unwrap_err(), ErrorMeta::SinMetaActiva);
        assert_eq!(resolver_meta(ComandoMeta::Lograr { turno_id: Uuid::new_v4() }, &estado, ahora()).unwrap_err(), ErrorMeta::SinMetaActiva);
        assert_eq!(resolver_meta(ComandoMeta::Fijar { texto: "  ".into() }, &estado, ahora()).unwrap_err(), ErrorMeta::TextoVacio);
        let fijada = resolver_meta(ComandoMeta::Fijar { texto: "meta".into() }, &estado, ahora()).expect("fija");
        assert_eq!(resolver_meta(ComandoMeta::Reanudar, &fijada.estado, ahora()).unwrap_err(), ErrorMeta::NoPausada);
        let pausada = resolver_meta(ComandoMeta::Pausar, &fijada.estado, ahora() + Duration::seconds(1)).expect("pausa");
        assert_eq!(resolver_meta(ComandoMeta::Pausar, &pausada.estado, ahora() + Duration::seconds(2)).unwrap_err(), ErrorMeta::YaPausada);
        assert_eq!(resolver_meta(ComandoMeta::Reanudar, &pausada.estado, ahora() - Duration::seconds(1)).unwrap_err(), ErrorMeta::FechaAnteriorAlInicio);
    }

    #[test]
    fn payload_sin_accion_conserva_el_contrato_del_panel() {
        assert_eq!(
            comando_desde_payload(Some("objetivo".into()), None, None).expect("fija"),
            ComandoMeta::Fijar {
                texto: "objetivo".into()
            }
        );
        assert_eq!(
            comando_desde_payload(Some("   ".into()), None, None).expect("limpia"),
            ComandoMeta::Limpiar
        );
        assert_eq!(
            comando_desde_payload(None, None, None).expect("limpia"),
            ComandoMeta::Limpiar
        );
    }

    #[test]
    fn payload_explicito_rechaza_combinaciones_ambiguas() {
        assert_eq!(
            comando_desde_payload(Some("texto".into()), Some("pausar"), None).unwrap_err(),
            ErrorMeta::ComandoInvalido("`pausar` no acepta `meta`".into())
        );
        assert_eq!(
            comando_desde_payload(None, Some("lograr"), None).unwrap_err(),
            ErrorMeta::TurnoRequerido
        );
        assert!(matches!(
            comando_desde_payload(None, Some("lograr"), Some("no-uuid")),
            Err(ErrorMeta::ComandoInvalido(_))
        ));
        assert!(matches!(
            comando_desde_payload(None, Some("bailar"), None),
            Err(ErrorMeta::ComandoInvalido(_))
        ));
        let turno = Uuid::new_v4();
        assert_eq!(
            comando_desde_payload(None, Some("lograr"), Some(&turno.to_string())).expect("logra"),
            ComandoMeta::Lograr { turno_id: turno }
        );
    }

    #[test]
    fn borrador_solo_admite_fijar_y_limpiar() {
        assert_eq!(
            aplicar_en_borrador(ComandoMeta::Fijar { texto: " meta ".into() }).expect("fija"),
            Some("meta".to_string())
        );
        assert_eq!(aplicar_en_borrador(ComandoMeta::Limpiar).expect("limpia"), None);
        assert_eq!(
            aplicar_en_borrador(ComandoMeta::Pausar).unwrap_err(),
            ErrorMeta::SinConversacion
        );
        assert_eq!(
            aplicar_en_borrador(ComandoMeta::Fijar { texto: "   ".into() }).unwrap_err(),
            ErrorMeta::TextoVacio
        );
    }
}

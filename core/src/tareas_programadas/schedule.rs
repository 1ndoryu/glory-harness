//! [119A-6 F2] `ScheduleTarea`: lenguaje canónico de programación de una
//! tarea (`manual | una_vez | intervalo | diario | entre_semana | semanal |
//! cron`) + zona horaria IANA. Sustituye al `cron_expr` suelto como fuente de
//! verdad para reprogramar: `cron::ejecutar_lista` y
//! `scheduler::ciclo_scheduler` calculan la próxima ejecución desde aquí
//! (tz-aware); el `cron_expr`/`tipo` heredados quedan como espejo legible.
//!
//! Texto canónico (lo que viaja en el puerto y la BD):
//! `<clase>:<expresion>@<Zona>` — p. ej. `cron:0 9 * * 1-5@Europe/Madrid`,
//! `intervalo:cada2h@UTC`, `una_vez:2026-09-12T09:00:00+02:00@Europe/Madrid`,
//! `manual@UTC` (sin expresión). La zona se valida en creación (fail-closed)
//! y el defecto es `UTC` explícito.
//!
//! Clases con expresión cron v2 (`M H * * DOW`, DOW admite rangos `1-5` y
//! listas `1,3,5` desde F2): `diario` (`M H` → `M H * * *`), `entre_semana`
//! (`M H` → `M H * * 1-5`), `semanal` (`M H DOW` → `M H * * DOW`), `cron`
//! (los 5 campos tal cual). `intervalo` usa `cada{N}min|h|d` (mínimo 1 min);
//! `una_vez` un RFC3339 con offset; `manual` no tiene expresión y nunca
//! vence (solo `schedule run <id>` la ejecuta).

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::scheduler;

/// Clase de programación (nombres del contrato Synara en español).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaseTarea {
    Manual,
    UnaVez,
    Intervalo,
    Diario,
    EntreSemana,
    Semanal,
    Cron,
}

impl ClaseTarea {
    /// Nombre canónico en el texto (`manual`, `una_vez`, …).
    #[must_use]
    pub fn nombre(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::UnaVez => "una_vez",
            Self::Intervalo => "intervalo",
            Self::Diario => "diario",
            Self::EntreSemana => "entre_semana",
            Self::Semanal => "semanal",
            Self::Cron => "cron",
        }
    }

    fn de_nombre(nombre: &str) -> Option<Self> {
        match nombre {
            "manual" => Some(Self::Manual),
            "una_vez" => Some(Self::UnaVez),
            "intervalo" => Some(Self::Intervalo),
            "diario" => Some(Self::Diario),
            "entre_semana" => Some(Self::EntreSemana),
            "semanal" => Some(Self::Semanal),
            "cron" => Some(Self::Cron),
            _ => None,
        }
    }

    /// ¿Vuelve a programarse sola tras ejecutarse? (`manual` y `una_vez` no.)
    #[must_use]
    pub fn repite(self) -> bool {
        !matches!(self, Self::Manual | Self::UnaVez)
    }
}

/// Programación validada y normalizada de una tarea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTarea {
    pub clase: ClaseTarea,
    /// Normalizada en `nueva`: "" (manual), RFC3339 (una_vez),
    /// `cada{N}min|h|d` (intervalo) o cron v2 completo (resto).
    pub expresion: String,
    pub zona_horaria: Tz,
}

impl ScheduleTarea {
    /// Construye y valida (zona IANA fail-closed; expresión según clase).
    pub fn nueva(clase: ClaseTarea, expresion: &str, zona: &str) -> Result<Self> {
        let zona_horaria: Tz = zona.parse().map_err(|_| {
            Error::Validacion(format!(
                "zona horaria desconocida '{zona}' (usa un nombre IANA como 'Europe/Madrid' o 'UTC')"
            ))
        })?;
        let expresion = expresion.trim();
        let normalizada = match clase {
            ClaseTarea::Manual => {
                if !expresion.is_empty() {
                    return Err(Error::Validacion(
                        "la programación 'manual' no lleva expresión".into(),
                    ));
                }
                String::new()
            }
            ClaseTarea::UnaVez => {
                DateTime::parse_from_rfc3339(expresion).map_err(|_| {
                    Error::Validacion(format!(
                        "una_vez exige fecha RFC3339 con offset (p. ej. '2026-09-12T09:00:00+02:00'), no '{expresion}'"
                    ))
                })?;
                expresion.to_string()
            }
            ClaseTarea::Intervalo => {
                let minusculas = expresion.to_ascii_lowercase();
                scheduler::validar_intervalo(&minusculas)?;
                minusculas
            }
            ClaseTarea::Diario => cron_desde_hora_o_cron(expresion, "*", false)?,
            ClaseTarea::EntreSemana => cron_desde_hora_o_cron(expresion, "1-5", false)?,
            ClaseTarea::Semanal => cron_desde_hora_o_cron(expresion, "", true)?,
            ClaseTarea::Cron => {
                let minusculas = expresion.to_ascii_lowercase();
                scheduler::validar_cron_v2(&minusculas)?;
                minusculas
            }
        };
        Ok(Self {
            clase,
            expresion: normalizada,
            zona_horaria,
        })
    }

    /// Reparsea un texto canónico (`texto()`); lo ya guardado se validó en
    /// creación, así que un error aquí es corrupción visible, no silencio.
    pub fn parse(texto: &str) -> Result<Self> {
        let (cuerpo, zona) = match texto.rsplit_once('@') {
            Some((c, z)) => (c, z),
            None => (texto, "UTC"),
        };
        let (clase_txt, expresion) = match cuerpo.split_once(':') {
            Some((c, e)) => (c, e),
            None => (cuerpo, ""),
        };
        let clase = ClaseTarea::de_nombre(clase_txt.trim()).ok_or_else(|| {
            Error::Validacion(format!(
                "programación desconocida '{texto}' (clases: manual, una_vez, intervalo, diario, entre_semana, semanal, cron)"
            ))
        })?;
        Self::nueva(clase, expresion, zona)
    }

    /// Texto canónico para puerto/BD/CLI (`clase:expresion@Zona`).
    #[must_use]
    pub fn texto(&self) -> String {
        if self.expresion.is_empty() {
            format!("{}@{}", self.clase.nombre(), self.zona_horaria.name())
        } else {
            format!(
                "{}:{}@{}",
                self.clase.nombre(),
                self.expresion,
                self.zona_horaria.name()
            )
        }
    }

    /// Próxima ejecución en UTC (`None` = `manual`, nunca vence sola).
    /// `una_vez` devuelve su instante aunque ya pasara (la columna
    /// `proxima_ejecucion` decide el vencimiento; tras correr no reprograma).
    pub fn proxima(&self, desde: DateTime<Utc>) -> Result<Option<DateTime<Utc>>> {
        match self.clase {
            ClaseTarea::Manual => Ok(None),
            ClaseTarea::UnaVez => {
                let instante = DateTime::parse_from_rfc3339(&self.expresion)
                    .map_err(|e| Error::Validacion(format!("una_vez corrupta: {e}")))?;
                Ok(Some(instante.with_timezone(&Utc)))
            }
            ClaseTarea::Intervalo => Ok(Some(scheduler::proxima_ejecucion(
                &self.expresion,
                desde,
            )?)),
            ClaseTarea::Diario
            | ClaseTarea::EntreSemana
            | ClaseTarea::Semanal
            | ClaseTarea::Cron => {
                let local = desde.with_timezone(&self.zona_horaria);
                let siguiente = scheduler::proxima_cron_v2(&self.expresion, local)?;
                Ok(Some(siguiente.with_timezone(&Utc)))
            }
        }
    }
}

/// `M H` → `M H * * <dow>` validado contra el subconjunto v2.
fn cron_desde_hora(expresion: &str, dow: &str) -> Result<String> {
    let campos: Vec<&str> = expresion.split_whitespace().collect();
    if campos.len() != 2 {
        return Err(Error::Validacion(format!(
            "se esperaban 'MINUTO HORA' (p. ej. '30 9'), no '{expresion}'"
        )));
    }
    let cron = format!("{} {} * * {dow}", campos[0], campos[1]);
    scheduler::validar_cron_v2(&cron)?;
    Ok(cron)
}

/// Acepta la forma corta `M H` (o `M H DOW` en semanal) o la ya
/// normalizada `M H * * DOW` (roundtrip `texto()` → `parse()`): la clase
/// fija el DOW permitido (`diario` → `*`, `entre_semana` → `1-5`, `semanal`
/// → un solo día 0-7). Cualquier otra forma pertenece a la clase `cron`.
fn cron_desde_hora_o_cron(expresion: &str, dow_esperado: &str, un_solo_dia: bool) -> Result<String> {
    let campos: Vec<&str> = expresion.split_whitespace().collect();
    if campos.len() == 5 {
        let (m, h, dom, mes, dow) = (campos[0], campos[1], campos[2], campos[3], campos[4]);
        let (dow_ok, etiqueta) = if un_solo_dia {
            (
                !dow.contains(['-', ',']) && dow.parse::<u32>().is_ok_and(|d| d <= 7),
                "un solo día (0-7)".to_string(),
            )
        } else {
            (dow == dow_esperado, format!("'{dow_esperado}'"))
        };
        if dom != "*" || mes != "*" || !dow_ok {
            return Err(Error::Validacion(format!(
                "cron de 5 campos incompatible con esta clase (DOW esperado {etiqueta}): '{expresion}' (usa clase 'cron')"
            )));
        }
        let cron = format!("{m} {h} * * {dow}");
        scheduler::validar_cron_v2(&cron)?;
        return Ok(cron);
    }
    if un_solo_dia {
        return cron_desde_hora_y_dow(expresion);
    }
    cron_desde_hora(expresion, dow_esperado)
}

/// `M H DOW` → `M H * * DOW` validado (un solo día; listas/rangos van en `cron`).
fn cron_desde_hora_y_dow(expresion: &str) -> Result<String> {    let campos: Vec<&str> = expresion.split_whitespace().collect();
    if campos.len() != 3 {
        return Err(Error::Validacion(format!(
            "semanal espera 'MINUTO HORA DIA' (p. ej. '0 9 1' = lunes), no '{expresion}'"
        )));
    }
    if campos[2].contains(['-', ',']) {
        return Err(Error::Validacion(
            "semanal admite un solo día (0-7); para varios usa clase 'cron'".into(),
        ));
    }
    let cron = format!("{} {} * * {}", campos[0], campos[1], campos[2]);
    scheduler::validar_cron_v2(&cron)?;
    Ok(cron)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn probe() -> DateTime<Utc> {
        // Domingo 2026-08-30 10:00 UTC (fijo: tests deterministas).
        Utc.with_ymd_and_hms(2026, 8, 30, 10, 0, 0).unwrap()
    }

    #[test]
    fn clases_y_texto_redondo() {
        let s = ScheduleTarea::nueva(
            ClaseTarea::Cron,
            "0 9 * * 1-5",
            "Europe/Madrid",
        )
        .expect("válido");
        assert_eq!(s.texto(), "cron:0 9 * * 1-5@Europe/Madrid");
        assert_eq!(ScheduleTarea::parse(&s.texto()).expect("reparse"), s);
        let manual = ScheduleTarea::nueva(ClaseTarea::Manual, "", "UTC").expect("válido");
        assert_eq!(manual.texto(), "manual@UTC");
        assert_eq!(manual.proxima(probe()).expect("proxima"), None);
    }

    #[test]
    fn zona_desconocida_falla_cerrado() {
        let err = ScheduleTarea::nueva(ClaseTarea::Diario, "0 9", "Atlantida/NoExiste")
            .expect_err("zona inválida");
        assert!(err.to_string().contains("zona horaria desconocida"));
    }

    #[test]
    fn diario_respeta_zona_y_dst() {
        // 09:00 en Madrid el 31-08-2026 (CEST, +2) = 07:00 UTC.
        let s =
            ScheduleTarea::nueva(ClaseTarea::Diario, "0 9", "Europe/Madrid").expect("válido");
        assert_eq!(s.expresion, "0 9 * * *");
        let prox = s.proxima(probe()).expect("proxima").expect("futura");
        assert_eq!(
            prox,
            Utc.with_ymd_and_hms(2026, 8, 31, 7, 0, 0).unwrap(),
            "las 9 de Madrid son las 7 UTC en verano"
        );
        // En invierno (+1) la misma clase da las 08:00 UTC.
        let desde_invierno = Utc.with_ymd_and_hms(2026, 11, 30, 10, 0, 0).unwrap();
        let prox_inv = s
            .proxima(desde_invierno)
            .expect("proxima")
            .expect("futura");
        assert_eq!(
            prox_inv,
            Utc.with_ymd_and_hms(2026, 12, 1, 8, 0, 0).unwrap(),
            "las 9 de Madrid son las 8 UTC en invierno (deriva con el DST)"
        );
    }

    #[test]
    fn entre_semana_expande_lunes_a_viernes() {
        let s =
            ScheduleTarea::nueva(ClaseTarea::EntreSemana, "0 9", "UTC").expect("válido");
        assert_eq!(s.expresion, "0 9 * * 1-5");
        // Domingo 30-08 10:00 UTC → lunes 31 a las 09:00 UTC.
        let prox = s.proxima(probe()).expect("proxima").expect("futura");
        assert_eq!(
            prox,
            Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap()
        );
        // Sábado 05-09 10:00 UTC → lunes 07-09 (salta el finde).
        let sabado = Utc.with_ymd_and_hms(2026, 9, 5, 10, 0, 0).unwrap();
        let prox2 = s.proxima(sabado).expect("proxima").expect("futura");
        assert_eq!(
            prox2,
            Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap()
        );
    }

    #[test]
    fn semanal_un_solo_dia() {
        let s = ScheduleTarea::nueva(ClaseTarea::Semanal, "0 9 1", "UTC").expect("válido");
        assert_eq!(s.expresion, "0 9 * * 1");
        assert!(
            ScheduleTarea::nueva(ClaseTarea::Semanal, "0 9 1-5", "UTC").is_err(),
            "semanal no acepta rangos (usa cron)"
        );
    }

    #[test]
    fn intervalo_minimo_un_minuto() {
        let s =
            ScheduleTarea::nueva(ClaseTarea::Intervalo, "cada2h", "UTC").expect("válido");
        let prox = s.proxima(probe()).expect("proxima").expect("futura");
        assert_eq!(prox, probe() + chrono::Duration::hours(2));
        assert!(
            ScheduleTarea::nueva(ClaseTarea::Intervalo, "cada30s", "UTC").is_err(),
            "sub-minuto se rechaza (granularidad 1 min)"
        );
        assert!(ScheduleTarea::nueva(ClaseTarea::Intervalo, "cada0h", "UTC").is_err());
    }

    #[test]
    fn una_vez_exige_rfc3339() {
        let s = ScheduleTarea::nueva(
            ClaseTarea::UnaVez,
            "2026-09-12T09:00:00+02:00",
            "Europe/Madrid",
        )
        .expect("válido");
        let prox = s.proxima(probe()).expect("proxima").expect("futura");
        assert_eq!(
            prox,
            Utc.with_ymd_and_hms(2026, 9, 12, 7, 0, 0).unwrap()
        );
        assert!(ScheduleTarea::nueva(ClaseTarea::UnaVez, "mañana a las 9", "UTC").is_err());
    }

    #[test]
    fn texto_parse_redondo_en_todas_las_clases() {
        // Regresión 119A-6: `texto()` normaliza a cron de 5 campos y
        // `parse()` debe aceptarlo (el ciclo y el cron reparsean lo guardado).
        let casos = [
            ("manual@UTC", "manual@UTC"),
            (
                "una_vez:2026-09-12T09:00:00+02:00@Europe/Madrid",
                "una_vez:2026-09-12T09:00:00+02:00@Europe/Madrid",
            ),
            ("intervalo:cada2h@UTC", "intervalo:cada2h@UTC"),
            ("diario:0 9@Europe/Madrid", "diario:0 9 * * *@Europe/Madrid"),
            ("entre_semana:30 8@UTC", "entre_semana:30 8 * * 1-5@UTC"),
            ("semanal:0 9 1@UTC", "semanal:0 9 * * 1@UTC"),
            ("cron:0 9 * * 1-5@UTC", "cron:0 9 * * 1-5@UTC"),
        ];
        for (entrada, canonico) in casos {
            let s = ScheduleTarea::parse(entrada).expect("parse válido");
            assert_eq!(s.texto(), canonico);
            let otra = ScheduleTarea::parse(&s.texto()).expect("roundtrip válido");
            assert_eq!(otra, s, "parse(texto()) es identidad en {entrada}");
        }
        assert!(
            ScheduleTarea::parse("diario:0 9 * * 1@UTC").is_err(),
            "diario no acepta DOW distinto de * (usa cron)"
        );
        assert!(
            ScheduleTarea::parse("semanal:0 9 * * 1-5@UTC").is_err(),
            "semanal no acepta rangos ni normalizados (usa cron)"
        );
    }
}

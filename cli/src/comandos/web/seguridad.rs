//! [139A-8 F1/K3+K9+K12] Endurecimiento del modo web (single-user local):
//! bind loopback sin token (K3), cookie `gh_sesion` firmada con secreto
//! aleatorio por arranque (K9) y límites de inicio de turnos (K12).
//!
//! Sin dependencias nuevas: la firma es SipHash-1-3 (`DefaultHasher`)
//! sobre `(secreto, sid)` —MAC por prefijo secreto de 128 bits— y el
//! rate-limit una ventana deslizante en memoria. La cookie firmada es defensa en profundidad: la sesión además
//! se valida contra el mapa en memoria (`autorizar_sesion`), que se vacía
//! al reiniciar, así que una cookie firmada de un arranque anterior nunca
//! autoriza nada (fail-closed).

use std::collections::VecDeque;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, DefaultHasher, Hash, Hasher};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use super::{AppState, ApiError, error};

/// [K3] Dirección de escucha: sin token maestro el servidor SOLO bindea
/// loopback (el modo tokenless es comodidad local, no una API sin auth
/// expuesta a la red); con token explícito se permite `0.0.0.0`.
pub(crate) fn direccion_escucha(hay_token: bool, puerto: u16) -> SocketAddr {
    if hay_token {
        SocketAddr::from(([0, 0, 0, 0], puerto))
    } else {
        SocketAddr::from(([127, 0, 0, 1], puerto))
    }
}

/// [K9] Variable de entorno para fijar el secreto de firma de la cookie
/// (alternativa al flag `--secreto-sesion`; el flag prevalece).
pub(crate) const ENV_SECRETO_SESION: &str = "GLORY_HARNESS_SESION_SECRETO";

/// [K9] Secreto de firma de la cookie `gh_sesion`: clave SipHash de 128
/// bits. Aleatorio por arranque (las cookies mueren con el proceso, igual
/// que el mapa de sesiones) o derivado de un texto para fijarlo en dev o
/// tras reinicios programados (`--secreto-sesion` / env).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SecretoSesion([u64; 2]);

impl SecretoSesion {
    /// Entropía por arranque desde el RNG del SO: cada `RandomState::new()`
    /// lleva claves aleatorias por proceso.
    pub(crate) fn aleatorio() -> Self {
        Self([
            RandomState::new().hash_one("glory-harness/sesion-secreto/a"),
            RandomState::new().hash_one("glory-harness/sesion-secreto/b"),
        ])
    }

    /// Deriva un secreto determinista de un texto (mismo texto → mismo
    /// secreto en cada arranque). La confidencialidad la aporta el texto,
    /// no el hash: no pasar secretos débiles o públicos.
    pub(crate) fn derivar(texto: &str) -> Self {
        let mut a = DefaultHasher::new();
        ("glory-harness/sesion/a", texto).hash(&mut a);
        let mut b = DefaultHasher::new();
        ("glory-harness/sesion/b", texto).hash(&mut b);
        Self([a.finish(), b.finish()])
    }

    /// Firma: SipHash-1-3 sobre `(secreto, sid)`. Las claves del hasher son
    /// fijas y públicas; la impredecibilidad la aportan los 128 bits del
    /// secreto (construcción MAC por prefijo secreto, sin deps nuevas).
    fn firma(&self, sid: &str) -> u64 {
        let mut h = DefaultHasher::new();
        (self.0[0], self.0[1], sid).hash(&mut h);
        h.finish()
    }

    /// Valor de cookie: `{sid}.{firma:016x}`.
    pub(crate) fn empaquetar(&self, sid: &str) -> String {
        format!("{sid}.{:016x}", self.firma(sid))
    }

    /// Verifica la firma en tiempo constante y devuelve el `sid`, o `None`
    /// ante cualquier manipulación o formato inválido (fail-closed: una
    /// cookie sin firmar —formato anterior— también se rechaza).
    pub(crate) fn desempaquetar(&self, valor: &str) -> Option<String> {
        let (sid, firma) = valor.rsplit_once('.')?;
        if sid.is_empty() {
            return None;
        }
        let esperada = format!("{:016x}", self.firma(sid));
        if esperada.len() != firma.len() {
            return None;
        }
        let mut diff = 0u8;
        for (a, b) in esperada.bytes().zip(firma.bytes()) {
            diff |= a ^ b;
        }
        if diff != 0 {
            return None;
        }
        Some(sid.to_string())
    }
}

/// [K12] Ventana deslizante de inicios de turno (global del proceso):
/// 30 inicios por minuto; el exceso es 429 `limite_turnos`.
pub(crate) const MAX_INICIOS_VENTANA: usize = 30;
pub(crate) const VENTANA_SECS: u64 = 60;
/// [K12] Tope de turnos ejecutándose a la vez en todo el proceso
/// (además del tope de 1 por sesión, que sigue devolviendo 409
/// `turno_activo`): el exceso es 429 `demasiados_turnos`.
pub(crate) const MAX_TURNOS_GLOBAL: usize = 4;

/// [K12] Control previo a `iniciar_turno`: registra el intento en la
/// ventana y comprueba el tope global de concurrencia. Se llama justo
/// después de autorizar, para acotar el trabajo por intento abusivo.
pub(crate) async fn control_inicio_turno(state: &AppState) -> Result<(), ApiError> {
    let ahora = Instant::now();
    {
        let mut v: VecDeque<Instant> = std::mem::take(&mut *state.inicios_turno.lock().await);
        v.retain(|t| ahora.duration_since(*t) <= Duration::from_secs(VENTANA_SECS));
        if v.len() >= MAX_INICIOS_VENTANA {
            *state.inicios_turno.lock().await = v;
            return Err(error(
                "limite_turnos",
                "demasiados inicios de turno: espera un minuto",
            ));
        }
        v.push_back(ahora);
        *state.inicios_turno.lock().await = v;
    }
    let mut activos = 0usize;
    for sesion in state.sesiones.lock().await.values() {
        if sesion.turno.lock().await.is_some() {
            activos += 1;
            if activos >= MAX_TURNOS_GLOBAL {
                // No se descuenta el intento de la ventana: el rechazo
                // también consume cuota (el abuso sigue siendo abuso).
                return Err(error(
                    "demasiados_turnos",
                    "demasiados turnos en curso: espera a que termine alguno",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estado_prueba() -> AppState {
        AppState {
            token: None,
            sesiones: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            fixture: true,
            secreto: SecretoSesion::derivar("test"),
            inicios_turno: tokio::sync::Mutex::new(VecDeque::new()),
        }
    }

    #[test]
    fn escucha_loopback_sin_token_y_todas_con_token() {
        assert_eq!(
            direccion_escucha(false, 8799),
            SocketAddr::from(([127, 0, 0, 1], 8799))
        );
        assert_eq!(
            direccion_escucha(true, 8799),
            SocketAddr::from(([0, 0, 0, 0], 8799))
        );
    }

    #[test]
    fn cookie_firmada_roundtrip() {
        let s = SecretoSesion::derivar("test");
        let sid = "12345678-1234-1234-1234-1234567890ab";
        let valor = s.empaquetar(sid);
        assert!(valor.starts_with(sid));
        assert_eq!(s.desempaquetar(&valor).as_deref(), Some(sid));
    }

    #[test]
    fn cookie_manipulada_o_sin_firmar_se_rechaza() {
        let s = SecretoSesion::derivar("test");
        let sid = "12345678-1234-1234-1234-1234567890ab";
        let valor = s.empaquetar(sid);
        // sid ajeno con la firma original
        let otro = valor.replacen("12345678", "87654321", 1);
        assert_eq!(s.desempaquetar(&otro), None);
        // firma alterada
        let mut rota = valor.clone();
        rota.pop();
        rota.push('0');
        assert_eq!(s.desempaquetar(&rota), None);
        // formato anterior (sid sin firmar), vacío y sin punto
        assert_eq!(s.desempaquetar(sid), None);
        assert_eq!(s.desempaquetar(""), None);
        assert_eq!(s.desempaquetar(".abcdef"), None);
        // otro secreto no verifica
        assert_eq!(SecretoSesion::derivar("otro").desempaquetar(&valor), None);
        // `derivar` es determinista: el mismo texto reabre la cookie
        assert_eq!(
            SecretoSesion::derivar("test").desempaquetar(&valor).as_deref(),
            Some(sid)
        );
    }

    #[tokio::test]
    async fn ventana_de_inicios_corta_en_429() {
        let state = estado_prueba();
        for _ in 0..MAX_INICIOS_VENTANA {
            state.inicios_turno.lock().await.push_back(Instant::now());
        }
        let err = control_inicio_turno(&state).await.expect_err("debe cortar");
        assert_eq!(err.code, "limite_turnos");
    }

    #[tokio::test]
    async fn primer_inicio_pasa_y_cuenta() {
        let state = estado_prueba();
        control_inicio_turno(&state).await.expect("primer inicio pasa");
        assert_eq!(state.inicios_turno.lock().await.len(), 1);
    }
}

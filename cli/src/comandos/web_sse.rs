//! [079A-1 F1] Difusión SSE lock-free: un mpsc acotado por suscriptor.
//!
//! Sustituye a `tokio::sync::broadcast` (cuyo `send` usa un `Mutex` interno
//! y bloquea workers bajo contención — incidente 096A,
//! regla `broadcast-mutex-riesgo-rs`). Cada suscriptor SSE recibe su propio
//! `mpsc::channel(256)`; emitir es `try_send` por suscriptor (lock-free,
//! sin `.await`, sin Mutex). Semántica conservada del broadcast:
//! buffer 256 y descarte ante lector lento (antes `Lagged`, ahora descarte
//! silencioso del cable para ese suscriptor).
//!
//! Limpieza: los receptores caídos se podan al suscribir (`is_closed`) y al
//! emitir (`try_send` fallido), así que una desconexión sin eventos
//! posteriores no deja entradas eternas (tope: 1 por reconexión huérfana,
//! podada en el próximo suscribir/emitir).

use std::collections::HashMap;

use tokio::sync::mpsc;

/// Capacidad por suscriptor (igual que el buffer del broadcast anterior).
const CAPACIDAD_SUSCRIPTOR: usize = 256;

/// Registro de suscriptores SSE de una sesión.
pub(crate) struct DifusionSse {
    siguiente: u64,
    suscriptores: HashMap<u64, mpsc::Sender<String>>,
}

impl DifusionSse {
    pub(crate) fn nueva() -> Self {
        Self {
            siguiente: 0,
            suscriptores: HashMap::new(),
        }
    }

    /// Registra un suscriptor y devuelve su id + receptor. Poda antes los
    /// receptores caídos para no acumular reconexiones huérfanas.
    pub(crate) fn suscribir(&mut self) -> (u64, mpsc::Receiver<String>) {
        self.suscriptores.retain(|_, tx| !tx.is_closed());
        let id = self.siguiente;
        self.siguiente = self.siguiente.wrapping_add(1);
        let (tx, rx) = mpsc::channel(CAPACIDAD_SUSCRIPTOR);
        self.suscriptores.insert(id, tx);
        (id, rx)
    }

    /// Da de baja un suscriptor (id desconocido = no-op).
    pub(crate) fn desuscribir(&mut self, id: u64) {
        self.suscriptores.remove(&id);
    }

    /// Emite el cable a todos los suscriptores (best-effort: lector lento o
    /// caído pierde el cable y se poda; nunca bloquea ni falla).
    pub(crate) fn emitir(&mut self, cable: String) {
        self.suscriptores
            .retain(|_, tx| tx.try_send(cable.clone()).is_ok() || !tx.is_closed());
    }

    #[cfg(test)]
    pub(crate) fn suscriptores_vivos(&self) -> usize {
        self.suscriptores.len()
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn suscribir_emitir_recibir() {
        let mut d = DifusionSse::nueva();
        let (_, mut rx) = d.suscribir();
        d.emitir("cable-1".to_string());
        assert_eq!(rx.try_recv().unwrap(), "cable-1");
    }

    #[test]
    fn lector_lento_no_bloquea_al_resto() {
        let mut d = DifusionSse::nueva();
        let (_, mut lento) = d.suscribir();
        let (_, mut rapido) = d.suscribir();
        for i in 0..(CAPACIDAD_SUSCRIPTOR + 10) {
            d.emitir(format!("cable-{i}"));
        }
        // El rápido recibe (el lento pierde cables pero emitir no falla).
        assert!(rapido.try_recv().is_ok());
        let _ = lento.try_recv();
    }

    #[test]
    fn receptor_caido_se_poda() {
        let mut d = DifusionSse::nueva();
        let (id, rx) = d.suscribir();
        drop(rx);
        d.emitir("cable".to_string());
        assert_eq!(d.suscriptores_vivos(), 0);
        // Desuscribir un id podado es no-op.
        d.desuscribir(id);
        assert_eq!(d.suscriptores_vivos(), 0);
    }
}

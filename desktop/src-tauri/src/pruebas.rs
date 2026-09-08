//! Tests del backend desktop (ventana P6-backend).

#![cfg(test)]

use super::*;

// [039A-3 P6-backend] La ventana del desktop defaultea a 150k y respeta
// el valor persistido; basura o valores bajo el piso → default.

fn memoria() -> PersistenciaSqlite {
    PersistenciaSqlite::en_memoria().expect("bd en memoria")
}

/// [039A-3 P6-backend] Ventana de contexto del desktop (default 150k, sin
/// tocar el default del core 128k): se lee de config y viaja en `OpcionesRun`
/// para inyectarse ANTES de construir el runtime. Ausente/ilegible/bajo el
/// piso → default del desktop; error de BD → se propaga (fail-closed).
fn leer_max_ventana(persistencia: &PersistenciaSqlite) -> Result<Option<u32>, String> {
    const VENTANA_DEFAULT_DESKTOP: u32 = 150_000;
    match persistencia
        .config_leer("contexto_max_ventana")
        .map_err(|e| e.to_string())?
    {
        Some(txt) => match txt.trim().parse::<u32>() {
            Ok(v) if v >= VENTANA_MINIMA => Ok(Some(v)),
            _ => Ok(Some(VENTANA_DEFAULT_DESKTOP)),
        },
        None => Ok(Some(VENTANA_DEFAULT_DESKTOP)),
    }
}

#[test]
fn sin_config_usa_el_default_del_desktop() {
    let p = memoria();
    assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
}

#[test]
fn respeta_el_valor_persistido() {
    let p = memoria();
    p.config_guardar("contexto_max_ventana", "200000")
        .expect("guarda");
    assert_eq!(leer_max_ventana(&p).expect("lee"), Some(200_000));
}

#[test]
fn basura_o_bajo_el_piso_cae_al_default() {
    let p = memoria();
    p.config_guardar("contexto_max_ventana", "no-numero")
        .expect("guarda");
    assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
    p.config_guardar("contexto_max_ventana", "5")
        .expect("guarda");
    assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
}

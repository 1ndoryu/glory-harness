//! Sanitizado de la memoria (diseño §4.2: sesgo a no guardar).

/// Palabras que, seguidas de `:` o `=` (con espacios intermedios), indican
/// credencial (`api_key = ...`, `token: ...`). Sin separador NO coincide:
/// "token" solo cuenta como asignación, nunca como palabra suelta (los turnos
/// hablan de "tokens" del modelo constantemente).
const CLAVES_SECRETO: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "api key",
    "secret",
    "passwd",
    "password",
    "contraseña",
    "contrasena",
    "token",
    "bearer",
    "private key",
    "aws_secret",
    "aws_access",
    "client_secret",
];

/// Prefijos de credencial que bastan por sí solos (`sk-...`, `ghp_...`).
const PREFIJOS_SECRETO: &[&str] = &["sk-", "ghp_", "gho_", "xoxa", "xoxb", "xoxp", "xoxs"];

/// ¿El texto parece contener un secreto? `true` → no se persiste.
#[must_use]
pub fn parece_secreto(texto: &str) -> bool {
    let minus = texto.to_lowercase();
    if minus.contains("-----begin") {
        return true; // Clave PEM.
    }
    if PREFIJOS_SECRETO.iter().any(|p| minus.contains(p)) {
        return true;
    }
    let bytes = minus.as_bytes();
    for clave in CLAVES_SECRETO {
        let mut desde = 0;
        while let Some(pos) = minus[desde..].find(clave) {
            let tras = &bytes[desde + pos + clave.len()..];
            let mut resto = tras.iter().peekable();
            while matches!(resto.peek(), Some(b' ' | b'\t')) {
                resto.next();
            }
            if matches!(resto.peek(), Some(b':') | Some(b'=')) {
                return true;
            }
            // `Bearer <valor>` (cabecera Authorization) no lleva separador:
            // si tras la palabra viene un token (racha ≥4 sin espacios),
            // es credencial. El umbral evita "bearer of bad news".
            if *clave == "bearer" {
                let racha: usize = resto.take_while(|b| !b.is_ascii_whitespace()).count();
                if racha >= 4 {
                    return true;
                }
            }
            desde += pos + clave.len();
        }
    }
    false
}

/// Limpia un candidato a recuerdo: recorta, descarta vacíos y secretos.
/// `None` = no persistir (vacío o parece credencial).
#[must_use]
pub fn sanitize_para_memoria(texto: &str) -> Option<String> {
    const MAX_CHARS: usize = 2000;
    let limpio = texto.trim();
    if limpio.is_empty() || parece_secreto(limpio) {
        return None;
    }
    if limpio.chars().count() > MAX_CHARS {
        Some(limpio.chars().take(MAX_CHARS).collect())
    } else {
        Some(limpio.to_string())
    }
}

#[cfg(test)]
mod pruebas {
    //! [069A-4] Secretos nunca se guardan.
    use super::*;

    #[test]
    fn secretos_por_asignacion_se_rechazan() {
        for texto in [
            "mi api_key = abc123",
            "token: xyz-secreto",
            "password= qwerty",
            "usa el Bearer abcdef",
            "aws_secret = AKIA...",
            "client_secret: s3cr3t",
        ] {
            assert!(parece_secreto(texto), "debería parecer secreto: {texto}");
            assert!(sanitize_para_memoria(texto).is_none());
        }
    }

    #[test]
    fn prefijos_y_pem_se_rechazan() {
        assert!(parece_secreto("la clave sk-abc123def"));
        assert!(parece_secreto("token ghp_xxYYzz1122"));
        assert!(parece_secreto("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn palabra_suelta_sin_asignacion_no_es_secreto() {
        // Los turnos hablan de "tokens" del modelo: sin `:`/`=` no hay
        // credencial y el recuerdo se conserva.
        for texto in [
            "cuenta los tokens usados en el turno",
            "el token de la sesión expiró y se renovó",
            "mi color favorito es el azul",
            "recuerda que prefiero respuestas concisas",
        ] {
            assert!(!parece_secreto(texto), "falso positivo: {texto}");
            assert!(sanitize_para_memoria(texto).is_some());
        }
    }

    #[test]
    fn vacio_y_recorte() {
        assert!(sanitize_para_memoria("   ").is_none());
        let largo = "x".repeat(3000);
        let recortado = sanitize_para_memoria(&largo).expect("no vacío");
        assert_eq!(recortado.chars().count(), 2000);
    }
}

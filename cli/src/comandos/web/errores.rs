//! Errores de la API web: cuerpo `{ ok, code, message }` y estado HTTP por código.

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};

#[derive(Debug, serde::Serialize)]
pub(crate) struct ApiError {
    ok: bool,
    pub(crate) code: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "no_autorizado" => StatusCode::UNAUTHORIZED,
            "origen" => StatusCode::FORBIDDEN,
            "no_encontrado" | "sin_turno" => StatusCode::NOT_FOUND,
            /* [109A-5 F1] Estados de meta que chocan con la petición (pausar
             * dos veces, reanudar sin pausa, lograr sin meta) o con el ciclo
             * de vida (borrador todavía sin conversación). */
            "turno_activo" | "sin_conversacion" | "sin_meta_activa" | "meta_ya_pausada"
            | "meta_no_pausada" => StatusCode::CONFLICT,
            "mensaje_largo" | "meta_larga" => StatusCode::PAYLOAD_TOO_LARGE,
            "demasiadas_sesiones" | "limite_turnos" | "demasiados_turnos" => {
                StatusCode::TOO_MANY_REQUESTS
            }
            "sesion_expirada" => StatusCode::GONE,
            "persistencia" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, Json(self)).into_response()
    }
}

pub(crate) fn error(code: &str, msg: impl Into<String>) -> ApiError {
    ApiError {
        ok: false,
        code: code.into(),
        message: msg.into(),
    }
}

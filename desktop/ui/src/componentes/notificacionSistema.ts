// Notificación del sistema (fix 12-09): toast de Windows cuando el agente
// necesita aprobación y el usuario puede estar en otra ventana. Solo existe
// en la app Tauri (`__TAURI__`); en web/mock es no-op silencioso. El permiso
// se pide una vez al arrancar; si se deniega, la tarjeta fija encima de la
// caja sigue siendo la vía visible (no se reintenta en cada petición).
import { esEntornoTauri } from '../tauri/real';

let permisoPedido = false;

/** Pide el permiso de notificaciones una sola vez (arranque, fire-and-forget). */
export async function pedirPermisoNotificaciones(): Promise<void> {
  if (!esEntornoTauri() || permisoPedido) return;
  permisoPedido = true;
  try {
    const { requestPermission } = await import('@tauri-apps/plugin-notification');
    await requestPermission();
  } catch {
    // Sin plugin o sin backend: la tarjeta fija sigue avisando en la app.
  }
}

/** Toast "aprobación requerida". Nunca lanza: fallar aquí no puede romper el turno. */
export async function notificarAprobacion(tool: string, detalle: string): Promise<void> {
  if (!esEntornoTauri()) return;
  try {
    const { isPermissionGranted, requestPermission, sendNotification } =
      await import('@tauri-apps/plugin-notification');
    if (!(await isPermissionGranted())) {
      if ((await requestPermission()) !== 'granted') return;
      permisoPedido = true;
    }
    await sendNotification({
      title: 'Glory Harness: aprobación requerida',
      body: `${tool} — ${detalle}`,
    });
  } catch {
    // La tarjeta fija encima de la caja sigue siendo la vía visible.
  }
}

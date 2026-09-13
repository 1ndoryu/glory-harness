import '../estilos/cambios.css';
import { el } from '../util/dom';
import { dentroDePrefijo, normalizarRutaArea } from '../util/reposUtil';
import { crearVueloUnico, type ModoVuelo } from '../util/vueloUnico';
import { consultarGit, crearGestorRepos } from './cambiosRepos';
import { montarPanelGit, type EstadoGit, type GitTransport, type PanelGit } from './panelGit';
import { crearVaultCambios } from './vaultCambios';
import type { CambioArchivoTurno, RestauracionArchivo } from '../tauri/realTipos';

/** Transporte del panel Cambios: vault por turno + rechazo puntual. */
export interface CambiosTransport {
  listar(conversacionId: string): Promise<CambioArchivoTurno[]>;
  rechazar(conversacionId: string, turnoId: string, ruta: string): Promise<RestauracionArchivo>;
}

export interface PanelCambios {
  raiz: HTMLElement;
  recargar(): void;
  /** [139A-7] Solo vault (cambiar de conversación): cero consultas a git. */
  recargarVault(): void;
  /** [129A-7 F3] Refresco en vivo tras una escritura del agente (el vault ya
   * la tiene: re-consulta con antirrebote y conserva la selección por ruta). */
  registrarCambioVivo(ruta: string, diff: string | null): void;
  /** [129A-8] Revela el archivo en el próximo pintado: lo selecciona, muestra
   * su diff y lo desplaza a la vista (en ambos modos: git o vault). */
  revelar(ruta: string): void;
}

/* [139A-1] Panel "Cambios": con git aplicable muestra SOLO el estado git (la
 * lista separada del vault era redundante); sin git muestra los cambios del
 * vault con las MISMAS clases git (`vaultCambios`). [139A-2] Con varios repos,
 * una sección por repo (cabecera `nombre · rama`) y los huérfanos en "Sin
 * repositorio". [139A-6] Repos y vault comparten `seccionColapsable` (planos,
 * minimizados por defecto; el revelado expande). [139A-7] Recargas
 * eficientes: single-flight con trailing, pipeline partido (cambiar de
 * conversación no toca git) y pintar-solo-si-cambió (firmas). */
export function montarPanelCambios(opts: {
  git: GitTransport;
  cambios: CambiosTransport;
  convId: () => string | null;
  onError?: (texto: string, detalle?: string) => void;
  onToast?: (texto: string, detalle?: string) => void;
}): PanelCambios {
  const raiz = el('div', 'panel-cambios');
  // [139A-2] Revalidación manual (el resto de refrescos son automáticos:
  // fin de turno, escritura viva, abrir la tab, cambio de área/conversación).
  const barra = el('div', 'cambios-barra');
  const btnActualizar = el('button', 'btn cambios-boton');
  btnActualizar.type = 'button';
  btnActualizar.textContent = 'Actualizar';
  btnActualizar.addEventListener('click', () => recargar());
  barra.appendChild(btnActualizar);
  const gitSimple: PanelGit = montarPanelGit({ transporte: opts.git, onError: opts.onError });
  gitSimple.raiz.hidden = true;

  let secuencia = 0;
  let debounce: ReturnType<typeof setTimeout> | null = null;
  const diffsVivos = new Map<string, string>();
  // [129A-8] Revelado pendiente (el listado es asíncrono: se aplica al pintar).
  let revelarPendiente: string | null = null;
  // [139A-7] Caché anti re-análisis: git cargado al menos una vez, prefijos
  // cubiertos, firmas del último pintado, conv pintada y sello de vivos (un
  // diff vivo nuevo fuerza el repintado del vault aunque el listado coincida).
  let gitListo = false;
  let cubiertosActuales: string[] = [];
  let firmaGitAnterior: string | null = null;
  let firmaVaultAnterior: string | null = null;
  let convPintada: string | null = null;
  let sellosVivos = 0;

  const gestor = crearGestorRepos({
    git: opts.git,
    onError: opts.onError,
    revelarHuerfano: (ruta) => vault.revelar(ruta),
  });

  const vault = crearVaultCambios({
    cambios: opts.cambios,
    diffsVivos,
    tomarRevelado: () => {
      const objetivo = revelarPendiente;
      revelarPendiente = null;
      return objetivo;
    },
    recargar: () => recargar(),
    onToast: opts.onToast,
  });
  raiz.append(barra, vault.raiz, gestor.raiz, gitSimple.raiz);

  function consumirRevelado(): string | null {
    const objetivo = revelarPendiente;
    revelarPendiente = null;
    return objetivo;
  }

  function firmaCambios(cambios: CambioArchivoTurno[]): string {
    return JSON.stringify(cambios.map((c) => [c.ruta, c.turno_id, c.en_ms, c.herramienta]));
  }

  /* Vault en modo multi-repo: huérfanos fuera de prefijos cubiertos. Solo
   * pinta si cambió algo visible (conv, lista, vivos o visibilidad git).
   * Devuelve el revelado consumido para aplicarlo sobre el DOM final. */
  async function pintarVaultRepos(id: number): Promise<string | null> {
    const conv = opts.convId();
    const ocultoGit = gestor.raiz.hidden;
    if (!conv) {
      convPintada = null;
      vault.ocultar();
      if (ocultoGit) vault.pintarVacio('sin cambios en el área');
      return null;
    }
    let cambios: CambioArchivoTurno[] = [];
    try {
      cambios = await opts.cambios.listar(conv);
    } catch (error: unknown) {
      if (id !== secuencia) return null;
      convPintada = null;
      vault.ocultar();
      opts.onError?.('no se pudieron listar los cambios', String(error));
      return null;
    }
    if (id !== secuencia) return null;
    const objetivo = consumirRevelado();
    const huerfanos = cambios.filter(
      (c) => !cubiertosActuales.some((prefijo) => dentroDePrefijo(normalizarRutaArea(c.ruta), prefijo)),
    );
    const firma = `R|${conv}|${sellosVivos}|${ocultoGit}|${firmaCambios(huerfanos)}`;
    if (convPintada !== conv || firma !== firmaVaultAnterior) {
      firmaVaultAnterior = firma;
      convPintada = conv;
      if (huerfanos.length > 0) {
        vault.pintar(huerfanos, conv, 'Sin repositorio', objetivo);
      } else {
        vault.ocultar();
        if (ocultoGit) vault.pintarVacio('sin cambios en el área');
      }
    }
    return objetivo;
  }

  async function ejecutar(modo: ModoVuelo): Promise<void> {
    const id = ++secuencia;
    // Vault-only (cambio de conversación): el revelado pendiente es de otra
    // conversación y se descarta; git ni se consulta. Sin git cargado aún,
    // cae al total para descubrir repos y prefijos cubiertos.
    if (modo === 'vault' && gitListo) {
      revelarPendiente = null;
      await pintarVaultRepos(id);
      return;
    }
    // [139A-2] Una consulta git por recarga total (batch `resumen` si el
    // transporte lo ofrece, si no fan-out); sin repos el camino simple 139A-1
    // queda intacto.
    let consulta: Awaited<ReturnType<typeof consultarGit>> | null = null;
    try {
      consulta = await consultarGit(opts.git);
    } catch {
      consulta = null;
    }
    if (id !== secuencia) return;
    if (consulta && consulta.repos.length > 0) {
      gitListo = true;
      gitSimple.raiz.hidden = true;
      gestor.sincronizar(consulta.repos);
      const { cubiertos, oculto } = gestor.pintar(consulta.resultados);
      cubiertosActuales = cubiertos;
      gestor.raiz.hidden = oculto;
      const objetivo = await pintarVaultRepos(id);
      if (id !== secuencia) return;
      if (objetivo) gestor.revelarAhora(normalizarRutaArea(objetivo));
      return;
    }
    gitListo = true;
    gestor.raiz.hidden = true;
    // Una sola consulta a git por recarga: decide el modo y pinta sin refetch.
    let gitEstado: EstadoGit | null = null;
    let gitError: string | null = null;
    try {
      gitEstado = await opts.git.estado();
    } catch (error: unknown) {
      gitError = String(error);
    }
    if (id !== secuencia) return;
    if (gitEstado?.aplicable) {
      vault.ocultar();
      gitSimple.raiz.hidden = false;
      convPintada = null;
      const firma = JSON.stringify(gitEstado);
      if (firma !== firmaGitAnterior) {
        firmaGitAnterior = firma;
        gitSimple.fijar(gitEstado);
      }
      const objetivo = consumirRevelado();
      if (objetivo && !gitSimple.seleccionar(objetivo)) gitSimple.seleccionar(normalizarRutaArea(objetivo));
      return;
    }
    if (gitError) opts.onError?.('no se pudo consultar Git', gitError);
    gitSimple.raiz.hidden = true;
    firmaGitAnterior = null;
    const objetivo = consumirRevelado();
    const conv = opts.convId();
    if (!conv) {
      convPintada = null;
      vault.pintarVacio('abre una conversación para ver sus cambios');
      return;
    }
    try {
      if (id !== secuencia) return;
      const cambios = await opts.cambios.listar(conv);
      if (id !== secuencia) return;
      const firma = `S|${conv}|${sellosVivos}|${firmaCambios(cambios)}`;
      if (convPintada !== conv || firma !== firmaVaultAnterior) {
        firmaVaultAnterior = firma;
        convPintada = conv;
        vault.pintar(cambios, conv, 'Cambios', objetivo);
      }
    } catch (error: unknown) {
      if (id !== secuencia) return;
      convPintada = null;
      vault.ocultar();
      opts.onError?.('no se pudieron listar los cambios', String(error));
    }
  }

  const vuelo = crearVueloUnico(ejecutar);

  function recargar(): void {
    vuelo.solicitar('total');
  }

  function recargarVault(): void {
    vuelo.solicitar('vault');
  }

  function registrarCambioVivo(ruta: string, diff: string | null): void {
    if (diff) {
      diffsVivos.set(ruta, diff);
      sellosVivos++;
    }
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(recargar, 800);
  }

  function revelar(ruta: string): void {
    revelarPendiente = ruta;
    recargar();
  }

  return { raiz, recargar, recargarVault, registrarCambioVivo, revelar };
}

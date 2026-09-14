import { el } from '../util/dom';
import { relativaEnRepo } from '../util/reposUtil';
import { crearSeccionColapsable, type SeccionColapsable } from '../util/seccionColapsable';
import {
  montarPanelGit,
  type GitTransport,
  type PanelGit,
  type RepoGit,
  type ResumenRepo,
} from './panelGit';

/* [139A-7] Bloque multi-repo del panel Cambios (extraído de `panelCambios`
 * por el límite de 300 líneas): una sección colapsable por repo con su
 * PanelGit, pintado con firma (si nada cambió no se toca el DOM) y consulta
 * batch con fallback al fan-out. */

/* Una sola consulta git por recarga total: batch `resumen` si el transporte
 * lo ofrece (F2: 1 IPC en vez de 1+N); si no, fan-out repos+estados. Si ni
 * `repos` responde (modo web), se propaga el fallo y el dueño usa el camino
 * simple 139A-1. */
export async function consultarGit(
  git: GitTransport,
): Promise<{ repos: RepoGit[]; resultados: ResumenRepo[] }> {
  if (git.resumen) {
    const resultados = await git.resumen();
    return { repos: resultados.map((r) => r.repo), resultados };
  }
  const repos = await git.repos();
  const resultados = await Promise.all(
    repos.map((repo) =>
      git.estado(repo.ruta).then(
        (est) => ({ repo, est, error: null as string | null }),
        (fallo: unknown) => ({ repo, est: null, error: String(fallo) }),
      ),
    ),
  );
  return { repos, resultados };
}

export interface GestorRepos {
  raiz: HTMLElement;
  /** Reconcilia secciones con los repos descubiertos (reutiliza wraps). */
  sincronizar(repos: RepoGit[]): void;
  /** Pinta estados; si la firma no cambió, no toca el DOM. Devuelve los
   * prefijos cubiertos y si el contenedor quedó oculto. */
  pintar(resultados: ResumenRepo[]): { cubiertos: string[]; oculto: boolean };
  /** [139A-2] Revelado: traduce la ruta del área a relativa al repo con el
   * prefijo más largo; fuera de todo repo delega en el huérfano. */
  revelarAhora(ruta: string): void;
}

export function crearGestorRepos(ctx: {
  git: GitTransport;
  onError?: (texto: string, detalle?: string) => void;
  revelarHuerfano: (ruta: string) => void;
}): GestorRepos {
  const raiz = el('div', 'panel-cambios-repos');
  raiz.hidden = true;
  const paneles = new Map<string, { repo: RepoGit; panel: PanelGit; seccion: SeccionColapsable }>();
  let vistos: RepoGit[] = [];
  let firmaAnterior: string | null = null;
  let cubiertosAnteriores: string[] = [];

  function panelPara(repo: RepoGit): {
    repo: RepoGit;
    panel: PanelGit;
    seccion: SeccionColapsable;
  } {
    const previo = paneles.get(repo.ruta);
    if (previo) {
      previo.repo = repo;
      return previo;
    }
    // [139A-6] La misma sección colapsable del vault (`nombre · rama` en el
    // título; `git-repo` solo marca el origen para consultas). El cuerpo es
    // el PanelGit: se pliega con la regla compartida `.seccion-cuerpo`.
    const seccion = crearSeccionColapsable();
    seccion.seccion.classList.add('git-repo');
    seccion.titulo.textContent = repo.nombre;
    const panel = montarPanelGit({ transporte: ctx.git, ruta: repo.ruta, onError: ctx.onError });
    panel.raiz.classList.add('seccion-cuerpo');
    seccion.seccion.appendChild(panel.raiz);
    raiz.appendChild(seccion.seccion);
    const entrada = { repo, panel, seccion };
    paneles.set(repo.ruta, entrada);
    return entrada;
  }

  function sincronizar(repos: RepoGit[]): void {
    vistos = repos;
    const vivas = new Set<string>();
    for (const repo of repos) {
      vivas.add(repo.ruta);
      raiz.appendChild(panelPara(repo).seccion.seccion);
    }
    for (const [ruta, entrada] of paneles) {
      if (!vivas.has(ruta)) {
        entrada.seccion.seccion.remove();
        paneles.delete(ruta);
      }
    }
  }

  function pintar(resultados: ResumenRepo[]): { cubiertos: string[]; oculto: boolean } {
    // La firma cubre repos + estados: si nada cambió, el DOM ya está bien.
    const firma = JSON.stringify(
      resultados.map(({ repo, est, error }) => [repo.ruta, repo.nombre, repo.prefijo, est, error]),
    );
    if (firma === firmaAnterior) return { cubiertos: cubiertosAnteriores, oculto: raiz.hidden };
    firmaAnterior = firma;
    // Prefijos con git legible: sus cambios del vault no se duplican.
    const cubiertos: string[] = [];
    for (const { repo, est, error } of resultados) {
      const entrada = paneles.get(repo.ruta);
      if (!entrada) continue;
      if (!est) {
        // Repo ilegible (p. ej. ownership dudoso): se oculta y sus cambios
        // del vault caen a "Sin repositorio" (nada se pierde).
        ctx.onError?.(`no se pudo consultar Git en ${repo.nombre}`, error ?? '');
        entrada.seccion.seccion.hidden = true;
        continue;
      }
      entrada.panel.fijar(est);
      // [139A-5] Rama unificada en el título (sin elemento suelto).
      entrada.seccion.titulo.textContent = est.rama ? `${repo.nombre} · ${est.rama}` : repo.nombre;
      const total = entrada.panel.raiz.querySelectorAll('.git-entrada').length;
      entrada.seccion.contador.textContent = String(total);
      entrada.seccion.seccion.hidden = total === 0 || !est.aplicable;
      if (est.aplicable) cubiertos.push(repo.prefijo);
    }
    cubiertosAnteriores = cubiertos;
    const visibles = [...paneles.values()].filter((e) => !e.seccion.seccion.hidden);
    // [Un solo repo] Minimizar el único visible no ahorra nada: queda fijo
    // (expandido y sin gesto); con varios, todos colapsables.
    const unico = visibles.length === 1 ? visibles[0] : null;
    for (const entrada of paneles.values()) {
      entrada.seccion.fijarFijo(entrada === unico);
    }
    return {
      cubiertos,
      oculto: visibles.length === 0,
    };
  }

  function revelarAhora(ruta: string): void {
    const ordenados = [...vistos].sort((a, b) => b.prefijo.length - a.prefijo.length);
    for (const repo of ordenados) {
      const relativa = relativaEnRepo(ruta, repo.prefijo);
      if (relativa === null) continue;
      const entrada = paneles.get(repo.ruta);
      if (entrada && !entrada.seccion.seccion.hidden) {
        // [139A-5] El revelado expande el repo (nace minimizado).
        entrada.seccion.fijarColapso(false);
        if (relativa !== '' && entrada.panel.seleccionar(relativa)) {
          entrada.seccion.seccion.scrollIntoView({ block: 'nearest' });
        }
      }
      // En repo cubierto no hay duplicado en huérfanos: terminar aquí.
      return;
    }
    ctx.revelarHuerfano(ruta);
  }

  return { raiz, sincronizar, pintar, revelarAhora };
}

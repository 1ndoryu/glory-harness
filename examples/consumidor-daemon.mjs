//! Segundo consumidor de Glory Harness (Fase 4): cliente del daemon NDJSON.
//!
//! Muestra el caso de uso de "consumir glory-harness como servicio de fondo":
//! en lugar de enlazar el crate como lib (primer consumidor: task), un proceso
//! externo se conecta al daemon por TCP/NDJSON en loopback y ejecuta turnos.
//!
//! Uso (tras arrancar el daemon con el token conocido):
//!   $env:GLORY_HARNESS_DAEMON_TOKEN="mi-token"
//!   glory-harness daemon --puerto 8798 --mostrar-token
//!   node examples/consumidor-daemon.mjs --token mi-token --mensaje "hola"
//!
//! Opciones:
//!   --puerto <n>   puerto del daemon (default 8798)
//!   --token <t>    token de autorización (default env GLORY_HARNESS_DAEMON_TOKEN)
//!   --mensaje <s>  mensaje del turno (default "hola")
//!
//! El stream recibido son líneas `AgenteEvento` (mismo contrato H3 que SSE) y
//! cierra con `{"tipo":"turno_done"}`. Un tiempo de espera y un manejo de error
//! evitan que el cliente quede colgado si el daemon no está disponible.

import net from "node:net";

function parseArgs(argv) {
  const args = { puerto: 8798, token: process.env.GLORY_HARNESS_DAEMON_TOKEN, mensaje: "hola" };
  for (let i = 0; i < argv.length; i++) {
    switch (argv[i]) {
      case "--puerto": args.puerto = Number(argv[++i]); break;
      case "--token": args.token = argv[++i]; break;
      case "--mensaje": args.mensaje = argv[++i]; break;
      default: break;
    }
  }
  return args;
}

function main() {
  const { puerto, token, mensaje } = parseArgs(process.argv.slice(2));
  if (!token) {
    console.error("[consumidor] falta --token <t> (o GLORY_HARNESS_DAEMON_TOKEN en env)");
    process.exit(2);
  }

  const cliente = net.createConnection({ host: "127.0.0.1", port: puerto }, () => {
    console.error(`[consumidor] conectado a 127.0.0.1:${puerto}`);
    cliente.write(JSON.stringify({ tipo: "sesion_abrir", token }) + "\n");
  });

  let buffer = "";
  let sessionId = null;
  let abierta = false;

  const timeout = setTimeout(() => {
    console.error("[consumidor] timeout: el daemon no respondió a tiempo");
    cliente.destroy();
    process.exit(1);
  }, 10000);

  cliente.on("data", (chunk) => {
    buffer += chunk.toString();
    let idx;
    while ((idx = buffer.indexOf("\n")) >= 0) {
      const linea = buffer.slice(0, idx);
      buffer = buffer.slice(idx + 1);
      if (!linea.trim()) continue;
      const msg = JSON.parse(linea);
      if (msg.tipo === "sesion_abierta") {
        sessionId = msg.session_id;
        abierta = true;
        console.error(`[consumidor] sesión abierta: ${sessionId}`);
        cliente.write(JSON.stringify({ tipo: "turno", session_id: sessionId, mensaje }) + "\n");
      } else if (msg.tipo === "turno_done") {
        console.error(`[consumidor] turno terminado: ${msg.turno_id}`);
        cliente.write(JSON.stringify({ tipo: "sesion_cerrar", session_id: sessionId }) + "\n");
      } else if (msg.tipo === "sesion_cerrada") {
        console.error("[consumidor] sesión cerrada; fin");
        clearTimeout(timeout);
        cliente.end();
      } else if (msg.tipo === "error") {
        console.error(`[consumidor] error del daemon: ${msg.mensaje}`);
        clearTimeout(timeout);
        cliente.end();
        process.exit(1);
      } else {
        // Evento del turno: se imprime crudo en stdout (texto/tools/contexto).
        console.log(JSON.stringify(msg));
      }
    }
  });

  cliente.on("close", () => process.exit(0));
  cliente.on("error", (err) => {
    console.error(`[consumidor] error de conexión: ${err.message}`);
    clearTimeout(timeout);
    process.exit(1);
  });
}

main();
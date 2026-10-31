import type { InstallContent } from "../schema";

const install = {
  meta: {
    title: "Instalación y despliegue — PingWAF",
    description:
      "Un comando instala PingWAF con Docker Compose: plano de control, agente integrado y PostgreSQL, con datos y certificados en volúmenes.",
  },
  hero: {
    eyebrow: "Instalación",
    title: "De cero a un nodo en marcha con un solo comando",
    lead: "Docker Compose es la vía recomendada: un comando inicia el plano de control, el agente integrado y PostgreSQL. La instalación por script sigue disponible para hosts con systemd.",
    badges: [
      { icon: "terminal", label: "Ejecución", value: "Un comando" },
      { icon: "box", label: "Modos", value: "all-in-one · server · agent" },
      { icon: "shield-check", label: "Plataforma", value: "Linux amd64 / arm64" },
      { icon: "clock", label: "Tiempo", value: "~2 minutos" },
    ],
    toc: [
      { id: "builder", label: "Generador de comandos" },
      { id: "interactive", label: "Instalación interactiva" },
      { id: "reference", label: "Referencia de flags" },
      { id: "troubleshooting", label: "Solución de problemas" },
    ],
    oneLiner:
      "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
  },
  builder: {
    id: "builder",
    eyebrow: "Generador de comandos",
    title: "Compón tu comando de instalación",
    lead: "Docker Compose está preseleccionado: un comando, base de datos incluida. Los campos ya traen los valores por defecto; cambia solo lo necesario. El script sigue en la segunda pestaña.",
    modes: [
      {
        id: "all-in-one",
        label: "All-in-one",
        body: "Plano de control y agente del plano de datos en un solo proceso en este host. Necesita un PostgreSQL 14+ accesible; el panel se sirve en :9080.",
        flags: ["--mode all-in-one", "--db-url postgres://pingwaf:•••@db:5432/pingwaf"],
      },
      {
        id: "server",
        label: "Server",
        body: "Solo el plano de control — panel, API y gRPC en :9080/:9090. Elígelo cuando los agentes de borde corran en hosts separados.",
        flags: ["--mode server", "--db-url postgres://…"],
      },
      {
        id: "agent",
        label: "Agent",
        body: "Un nodo del plano de datos que proxifica el tráfico y se conecta al plano de control por gRPC. No guarda base de datos; necesita --server-url y una clave de alta.",
        flags: ["--mode agent", "--server-url https://cp:9090", "--api-key pwk_…"],
      },
    ],
    fields: [
      {
        key: "version",
        label: "Versión",
        placeholder: "p. ej. 0.33.0 (vacío = última versión)",
        hint: "Fija una versión sin la v inicial. 'latest' (el valor prefillado por defecto) o vacío instala la última publicación.",
        modes: ["all-in-one", "server", "agent"],
        optional: true,
      },
      {
        key: "dbUrl",
        label: "URL de PostgreSQL",
        placeholder: "postgres://pingwaf:pingwaf@localhost:5432/pingwaf",
        hint: "Cadena de conexión de la base de datos del plano de control. El esquema se crea en el primer arranque.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "serverUrl",
        label: "URL del plano de control",
        placeholder: "https://control-plane.example.com:9090",
        hint: "Endpoint gRPC del plano de control. Usa https:// — el TLS de gRPC está activo por defecto; descarga la CA en Ajustes → Control plane HTTPS.",
        modes: ["agent"],
      },
      {
        key: "apiKey",
        label: "Clave API de alta",
        placeholder: "pwk_…",
        hint: "Token de alta del agente emitido por el plano de control. Prefiere la variable de entorno PINGWAF_API_KEY para que no quede en el historial del shell.",
        modes: ["agent"],
        secret: true,
      },
      {
        key: "adminEmail",
        label: "Correo del administrador",
        placeholder: "ops@example.com",
        hint: "Cuenta de administrador sembrada. Por defecto es admin@pingwaf.local si se deja vacío.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "adminPassword",
        label: "Contraseña del administrador",
        placeholder: "••••••••",
        hint: "Opcional — déjala vacía para conservar la contraseña documentada (pingwaf123); el panel obliga a cambiarla en el primer inicio de sesión. Por eso el generador nunca la escribe en el comando.",
        modes: ["all-in-one", "server"],
        optional: true,
        secret: true,
      },
      {
        key: "tlsSans",
        label: "Nombres del certificado (SAN)",
        placeholder: "waf.example.com, waf.internal",
        hint: "Nombres de host separados por comas que debe cubrir el certificado generado del panel. Vacío cubre el nombre que uses para llegar al panel.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
    ],
    optionalLabel: "opcional",
    defaultsLabel:
      "Prefillado con los valores por defecto del instalador — solo los valores que cambies se añaden al comando.",
    resetLabel: "Restaurar valores por defecto",
    commandTitle: "Tu comando de instalación",
    note: "Solo los valores distintos de los predeterminados se añaden como flags, por lo que el generador intacto produce la línea más corta. Los argumentos llegan al script canalizado mediante bash -s --.",
    secretWarning:
      "La clave API aparece en texto plano en el comando y puede acabar en el historial del shell. Prefiere exportar PINGWAF_API_KEY, o ejecuta el instalador de forma interactiva y pega la clave cuando la pida.",
    copy: "Copiar",
    copied: "Copiado",
    emptyHint:
      "El modo agent aún necesita la URL del plano de control y una clave de alta — el comando se genera, pero el agente no podrá registrarse hasta que se rellenen ambos.",
    scriptUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh",
  },
  deployment: {
    tabs: { compose: "Docker Compose", script: "Script de instalación" },
    compose: {
      tabLabel: "Docker Compose",
      recommendedBadge: "Recomendado",
      installerUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh",
      oneLiner: "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
      oneLinerTitle: "Instalación en una línea",
      oneLinerNote: "Ejecútalo en un directorio vacío: el instalador descarga docker-compose.yml, escribe un .env privado, descarga la imagen e inicia el stack. Un .env existente nunca se sobrescribe: vuelve a ejecutar el mismo comando para actualizar.",
      customizeTitle: "¿Necesitas personalizar? Genera un comando con opciones",
      customizeHint: "Cambia solo lo necesario; el comando añade únicamente las opciones que difieren de los valores por defecto. La contraseña del administrador nunca se escribe en el comando.",
      title: "Despliega con Docker Compose",
      lead: "Docker Compose es la vía recomendada: un comando inicia el plano de control, su agente integrado y PostgreSQL, con datos y certificados en volúmenes. Actualizar solo cambia la etiqueta de la imagen.",
      steps: [
        {
          title: "Ejecuta el comando",
          body: "Ejecuta el comando anterior en un directorio vacío. El instalador crea ./pingwaf, descarga el compose y escribe un .env privado.",
        },
        {
          title: "Espera al stack",
          body: "Compose descarga la imagen, inicia PostgreSQL, espera su health check y arranca PingWAF. El primer arranque tarda uno o dos minutos.",
        },
        {
          title: "Inicia sesión",
          body: "Abre https://<host>:9080 y usa ADMIN_EMAIL / ADMIN_PASSWORD (la contraseña por defecto debe cambiarse al entrar). Después añade un sitio.",
        }
      ],
      fields: [
        {
          key: "tag",
          label: "Etiqueta de imagen",
          placeholder: "latest",
          hint: "Etiqueta de la imagen ghcr.io/shuaizend/pingwaf. 'latest' sigue la última publicación; fija una versión publicada (p. ej. 0.33.0) en producción.",
          defaultValue: "latest",
        },
        {
          key: "adminEmail",
          label: "Correo del administrador",
          placeholder: "admin@pingwaf.local",
          hint: "Cuenta de administrador sembrada. Por defecto: admin@pingwaf.local.",
          defaultValue: "admin@pingwaf.local",
        },
        {
          key: "tlsSans",
          label: "Nombres del certificado (SAN)",
          placeholder: "waf.example.com",
          hint: "Nombres de host separados por comas que debe cubrir el certificado autofirmado del panel. Vacío cubre el nombre que uses para llegar al panel.",
          defaultValue: "",
        },
        {
          key: "httpPort",
          label: "Puerto HTTP (host)",
          placeholder: "80",
          hint: "Puerto del host para el tráfico HTTP proxificado. Cámbialo si el 80 ya está ocupado; el puerto dentro del contenedor sigue siendo 80.",
          defaultValue: "80",
        },
        {
          key: "httpsPort",
          label: "Puerto HTTPS (host)",
          placeholder: "443",
          hint: "Puerto del host para el tráfico HTTPS proxificado. Cámbialo si el 443 ya está ocupado; el puerto dentro del contenedor sigue siendo 443.",
          defaultValue: "443",
        },
        {
          key: "adminPort",
          label: "Puerto del panel (host)",
          placeholder: "9080",
          hint: "Puerto del host para el panel y la API REST. El gRPC de los agentes sigue en :9090.",
          defaultValue: "9080",
        },
      ],
      envTitle: ".env generado",
      commandTitle: "Arrancar la pila",
      note: "Primer inicio de sesión: https://<tu-host>:9080 con el ADMIN_EMAIL / ADMIN_PASSWORD de arriba (acepta el aviso del certificado autofirmado hasta que subas uno real). Los agentes alcanzan el plano de control en :9090.",
      upgradeTitle: "Actualización",
      upgradeBody:
        "Descarga la imagen de la etiqueta de tu .env y recrea el contenedor con los mismos volúmenes — configuración, datos y certificados sobreviven al reinicio. Fija la etiqueta cuando quieras actualizaciones controladas.",
      upgradeCommand: "docker compose pull && docker compose up -d",
      requirements: [
        "Docker Engine 24+ con el plugin Compose v2 (docker compose, no el binario antiguo docker-compose).",
        "Puertos 80, 443, 9080 y 9090 libres en el host — o cámbialos en los campos de abajo.",
        "No hace falta una base de datos externa: el archivo compose incluye su propio contenedor PostgreSQL 16 con volúmenes con nombre.",
        "El panel sirve HTTPS con un certificado autofirmado en el primer arranque. Sube uno real en Ajustes → Control plane HTTPS, o pon TLS_ENABLED=false para HTTP plano.",
      ],
      screenshot: {
        key: "installCompose",
        alt: "Terminal mostrando docker compose up -d creando la red y los volúmenes, arrancando PostgreSQL y PingWAF e imprimiendo la URL del panel",
        caption:
          "docker compose up -d descarga la imagen, arranca PostgreSQL, espera a que esté healthy y levanta el panel.",
      },
    },
  },
  interactive: {
    id: "interactive",
    eyebrow: "Instalación interactiva",
    title: "O deja que el instalador pregunte",
    lead: "Ejecuta el script en una terminal sin flags y te guiará por el modo, las credenciales, la base de datos y los nombres de certificado; al final imprime el plan resuelto para confirmar.",
    steps: [
      {
        title: "Elige el modo de despliegue",
        body: "1 all-in-one, 2 server o 3 agent — pulsa Enter para aceptar el valor por defecto entre corchetes.",
      },
      {
        title: "Rellena los parámetros",
        body: "Los agentes preguntan por la URL del plano de control y la clave de alta; los modos server preguntan por la cuenta de administrador, la URL de PostgreSQL y los nombres de certificado. Los secretos no se muestran al escribirlos.",
      },
      {
        title: "Confirma el resumen",
        body: "Antes de escribir nada, el instalador imprime el plan resuelto — modo, versión, plataforma, rutas, estado de systemd y la URL de base de datos enmascarada.",
      },
      {
        title: "Termina y verifica",
        body: "Los modos server imprimen el comando de arranque; los agentes se habilitan y arrancan automáticamente. Confirma el nodo en Nodes del panel.",
      },
    ],
    terminalNote:
      "El modo interactivo necesita una terminal local. Con curl | bash la stdin es el propio script — descárgalo primero y ejecútalo con bash install.sh para responder a las preguntas.",
    shots: [
      {
        key: "installInteractive",
        alt: "Instalador interactivo preguntando por el modo, la cuenta de administrador, la URL de PostgreSQL y los nombres de certificado, terminando en el plan de instalación",
        caption:
          "El instalador interactivo pregunta en orden e imprime el plan resuelto antes de escribir nada.",
      },
      {
        key: "installSuccess",
        alt: "Banner de éxito del instalador con las rutas del binario, la configuración, los datos y el servicio, más los siguientes pasos",
        caption:
          "Una ejecución correcta registra pingwaf.service e imprime dónde quedó cada cosa.",
      },
    ],
  },
  reference: {
    id: "reference",
    eyebrow: "Referencia de flags",
    title: "Los flags del instalador de un vistazo",
    lead: "Todos los flags que acepta install.sh. Cada uno tiene su equivalente en la variable de entorno PINGWAF_*.",
    rows: [
      {
        flag: "--mode",
        description:
          "Modo de operación: all-in-one, server o agent. Por defecto all-in-one.",
      },
      {
        flag: "--version",
        description:
          "Instala una versión concreta (se elimina la v inicial). Vacío o 'latest' resuelve la última publicación de GitHub.",
      },
      {
        flag: "--db-url",
        description:
          "Cadena de conexión de PostgreSQL para server/all-in-one. Por defecto postgres://pingwaf:pingwaf@localhost:5432/pingwaf.",
      },
      {
        flag: "--server-url",
        description:
          "URL gRPC del plano de control para el modo agent. Usa https:// — gRPC se sirve con TLS por defecto.",
      },
      {
        flag: "--api-key",
        description:
          "Token de alta del agente. También puede pasarse como PINGWAF_API_KEY para no dejarlo en la línea de comandos.",
      },
      {
        flag: "--admin-email",
        description:
          "Correo del administrador sembrado (server/all-in-one). Por defecto admin@pingwaf.local.",
      },
      {
        flag: "--admin-password",
        description:
          "Contraseña del administrador sembrado (server/all-in-one). Vacío conserva pingwaf123, que el panel obliga a cambiar en el primer inicio de sesión.",
      },
      {
        flag: "--tls-sans",
        description:
          "Nombres de host separados por comas que debe cubrir el certificado generado del panel.",
      },
      {
        flag: "--no-systemd",
        description:
          "No crear la unidad de systemd — para contenedores, actualizaciones o gestores de procesos propios.",
      },
      {
        flag: "--interactive / --non-interactive",
        description:
          "Fuerza las preguntas aunque se hayan pasado flags, o nunca pregunta y usa los valores dados más los por defecto. Canalizarlo sin flags sigue siendo no interactivo.",
      },
      {
        flag: "--dry-run",
        description:
          "Imprime el plan resuelto — modo, versión, plataforma, rutas — y sale sin instalar.",
      },
    ],
    link: {
      label: "Manual de CLI en GitHub",
      path: "https://github.com/shuaiZend/PingWAF/blob/main/docs/cli.md",
    },
  },
  troubleshooting: {
    id: "troubleshooting",
    eyebrow: "Solución de problemas",
    title: "Cuando la instalación no termina limpia",
    lead: "Los cuatro fallos más frecuentes en soporte y el comando que identifica cada uno.",
    items: [
      {
        q: "El instalador no puede conectar con PostgreSQL",
        a: "Los modos all-in-one y server necesitan PostgreSQL 14+ antes del primer arranque. El script avisa cuando psql no puede conectar — instala el servidor, crea el usuario y la base de datos pingwaf (createuser / createdb), vuelve a ejecutar el instalador o edita /etc/pingwaf/pingwaf.toml y ejecuta systemctl restart pingwaf.",
      },
      {
        q: "bind() failed en 0.0.0.0:80 o :443",
        a: "El plano de datos enlaza 80/443 como el usuario sin privilegios pingwaf, lo que solo funciona porque la unidad otorga CAP_NET_BIND_SERVICE. Si el enlace sigue fallando, comprueba systemctl show pingwaf -p AmbientCapabilities y añade la capacidad con un drop-in: sudo systemctl edit pingwaf → [Service] AmbientCapabilities=CAP_NET_BIND_SERVICE. Las unidades escritas antes de otorgar la capacidad siguen fallando hasta añadir el drop-in.",
      },
      {
        q: "El binario falla con un error de símbolo glibc con versión",
        a: "Los artefactos de publicación se compilan en Ubuntu 24.04 y necesitan glibc 2.39 o superior. En distribuciones más antiguas (Debian 11, Ubuntu 20.04, CentOS/RHEL, Alpine, …) la prueba de humo del instalador se detiene con un mensaje claro — compila desde el código fuente en su lugar.",
      },
      {
        q: "El servicio de systemd no arranca",
        a: "Inspecciona el registro de la unidad con journalctl -u pingwaf -e y revisa /etc/pingwaf/pingwaf.toml. No hay recarga: aplica cualquier cambio con systemctl restart pingwaf y comprueba después con systemctl status pingwaf.",
      },
    ],
  },
  cta: {
    title: "¿Instalado? Ahora añade tu primer sitio.",
    lead: "La documentación cubre el modelo de sitios, las topologías distribuidas, la precedencia de configuración y la referencia completa de variables de entorno.",
    primary: { label: "Leer la documentación", path: "/docs/" },
    secondary: {
      label: "Releases en GitHub",
      path: "https://github.com/shuaiZend/PingWAF/releases",
    },
  },
} satisfies InstallContent;

export default install;

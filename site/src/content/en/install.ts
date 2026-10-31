import type { InstallContent } from "../schema";

const install = {
  meta: {
    title: "Install & deploy — PingWAF",
    description:
      "One command installs PingWAF with Docker Compose: control plane, embedded agent and PostgreSQL, with data and certificates on named volumes.",
  },
  hero: {
    eyebrow: "Install",
    title: "From zero to a running node in one command",
    lead: "Docker Compose is the recommended install: one command starts the control plane, embedded agent and PostgreSQL. The script install remains available for hosts managed by systemd.",
    badges: [
      { icon: "terminal", label: "Run", value: "One command" },
      { icon: "box", label: "Modes", value: "all-in-one · server · agent" },
      { icon: "shield-check", label: "Platform", value: "Linux amd64 / arm64" },
      { icon: "clock", label: "Time", value: "~2 minutes" },
    ],
    toc: [
      { id: "builder", label: "Command builder" },
      { id: "interactive", label: "Interactive setup" },
      { id: "reference", label: "Flag reference" },
      { id: "troubleshooting", label: "Troubleshooting" },
    ],
    oneLiner:
      "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
  },
  builder: {
    id: "builder",
    eyebrow: "Command builder",
    title: "Compose your install command",
    lead: "Docker Compose is pre-selected — one command, database included. Fields are pre-filled with the documented defaults; change only what you need. Script install remains on the second tab.",
    modes: [
      {
        id: "all-in-one",
        label: "All-in-one",
        body: "Control plane and data-plane agent in one process on this host. Needs a reachable PostgreSQL 14+; the dashboard is served on :9080.",
        flags: ["--mode all-in-one", "--db-url postgres://pingwaf:•••@db:5432/pingwaf"],
      },
      {
        id: "server",
        label: "Server",
        body: "Control plane only — dashboard, API and gRPC on :9080/:9090. Choose this when the edge agents run on separate hosts.",
        flags: ["--mode server", "--db-url postgres://…"],
      },
      {
        id: "agent",
        label: "Agent",
        body: "A data-plane node that proxies traffic and connects to a control plane over gRPC. Keeps no database; needs --server-url and an enrollment key.",
        flags: ["--mode agent", "--server-url https://cp:9090", "--api-key pwk_…"],
      },
    ],
    fields: [
      {
        key: "version",
        label: "Version",
        placeholder: "e.g. 0.33.0 (empty = latest release)",
        hint: "Pin a release without the leading v. 'latest' (the pre-filled default) or empty installs the newest published release.",
        modes: ["all-in-one", "server", "agent"],
        optional: true,
      },
      {
        key: "dbUrl",
        label: "PostgreSQL URL",
        placeholder: "postgres://pingwaf:pingwaf@localhost:5432/pingwaf",
        hint: "Connection string for the control plane database. The schema is created on first boot.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "serverUrl",
        label: "Control plane URL",
        placeholder: "https://control-plane.example.com:9090",
        hint: "gRPC endpoint of the control plane. Use https:// — gRPC TLS is on by default; download the CA under Settings → Control plane HTTPS.",
        modes: ["agent"],
      },
      {
        key: "apiKey",
        label: "Enrollment API key",
        placeholder: "pwk_…",
        hint: "Agent enrollment token from the control plane. Prefer the PINGWAF_API_KEY environment variable so it stays out of shell history.",
        modes: ["agent"],
        secret: true,
      },
      {
        key: "adminEmail",
        label: "Administrator e-mail",
        placeholder: "ops@example.com",
        hint: "Seeded administrator account. Defaults to admin@pingwaf.local when empty.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "adminPassword",
        label: "Administrator password",
        placeholder: "••••••••",
        hint: "Optional — leave empty to keep the documented default (pingwaf123); the console forces a change at first sign-in. The builder therefore never writes it into the command.",
        modes: ["all-in-one", "server"],
        optional: true,
        secret: true,
      },
      {
        key: "tlsSans",
        label: "Certificate names (SANs)",
        placeholder: "waf.example.com, waf.internal",
        hint: "Comma-separated hostnames the generated dashboard certificate should cover. Empty lets it cover whatever name you use to reach the console.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
    ],
    optionalLabel: "optional",
    defaultsLabel:
      "Pre-filled with the installer defaults — only values you change are added to the command.",
    resetLabel: "Reset to defaults",
    commandTitle: "Your install command",
    note: "Only values that differ from the defaults are appended as flags, so the untouched builder produces the shortest one-liner. Arguments reach the piped script through bash -s --.",
    secretWarning:
      "The API key appears in plain text in the command and may end up in your shell history. Prefer exporting PINGWAF_API_KEY instead, or run the installer interactively and paste the key when asked.",
    copy: "Copy",
    copied: "Copied",
    emptyHint:
      "Agent mode still needs a control plane URL and an enrollment key — the command is generated, but the agent cannot register until both are provided.",
    scriptUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh",
  },
  deployment: {
    tabs: { compose: "Docker Compose", script: "Install script" },
    compose: {
      tabLabel: "Docker Compose",
      recommendedBadge: "Recommended",
      installerUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh",
      oneLiner: "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
      oneLinerTitle: "One-line install",
      oneLinerNote: "Run it in an empty directory: the installer downloads docker-compose.yml, writes a private .env, pulls the image and starts the stack. An existing .env is never overwritten — re-run the same command to upgrade.",
      customizeTitle: "Need to customise? Generate a command with flags",
      customizeHint: "Only change what you need; the command appends flags that differ from the defaults. The administrator password is never written into the command — the console forces a change at first sign-in.",
      title: "Deploy with Docker Compose",
      lead: "Docker Compose is the recommended path: one command starts the control plane, its embedded agent and PostgreSQL, with data and certificates in named volumes. Upgrades only change the image tag.",
      steps: [
        {
          title: "Run the one-liner",
          body: "Run the command above in an empty directory. The installer creates ./pingwaf, downloads the compose file and writes a private .env.",
        },
        {
          title: "Wait for the stack",
          body: "Compose pulls the image, starts PostgreSQL, waits for its health check and then starts PingWAF. The first boot takes a minute or two.",
        },
        {
          title: "Sign in",
          body: "Open https://<host>:9080 and use ADMIN_EMAIL / ADMIN_PASSWORD (the default password must be changed at first sign-in). Then add a site.",
        }
      ],
      fields: [
        {
          key: "tag",
          label: "Image tag",
          placeholder: "latest",
          hint: "Image tag of ghcr.io/shuaizend/pingwaf. 'latest' tracks the newest release; pin a released version (e.g. 0.33.0) for production.",
          defaultValue: "latest",
        },
        {
          key: "adminEmail",
          label: "Administrator e-mail",
          placeholder: "admin@pingwaf.local",
          hint: "Seeded administrator account. Default: admin@pingwaf.local.",
          defaultValue: "admin@pingwaf.local",
        },
        {
          key: "tlsSans",
          label: "Certificate names (SANs)",
          placeholder: "waf.example.com",
          hint: "Comma-separated hostnames the self-signed dashboard certificate should cover. Empty covers whatever name you use to reach the console.",
          defaultValue: "",
        },
        {
          key: "httpPort",
          label: "HTTP port (host)",
          placeholder: "80",
          hint: "Host port for proxied HTTP traffic. Override it when 80 is already taken; the port inside the container stays 80.",
          defaultValue: "80",
        },
        {
          key: "httpsPort",
          label: "HTTPS port (host)",
          placeholder: "443",
          hint: "Host port for proxied HTTPS traffic. Override it when 443 is already taken; the port inside the container stays 443.",
          defaultValue: "443",
        },
        {
          key: "adminPort",
          label: "Dashboard port (host)",
          placeholder: "9080",
          hint: "Host port for the dashboard and REST API. gRPC for agents stays on :9090.",
          defaultValue: "9080",
        },
      ],
      envTitle: "Generated .env",
      commandTitle: "Start the stack",
      note: "First login: https://<your-host>:9080 with the ADMIN_EMAIL / ADMIN_PASSWORD above (accept the self-signed certificate warning until you upload a real one). Agents reach the control plane on :9090.",
      upgradeTitle: "Upgrading",
      upgradeBody:
        "Pulls the image for the tag in your .env and recreates the container with the same volumes — configuration, data and certificates survive the restart. Pin the tag when you want controlled upgrades.",
      upgradeCommand: "docker compose pull && docker compose up -d",
      requirements: [
        "Docker Engine 24+ with the Compose v2 plugin (docker compose, not the legacy docker-compose binary).",
        "Ports 80, 443, 9080 and 9090 free on the host — or override them in the fields below.",
        "No external database needed: the compose file ships its own PostgreSQL 16 container with named volumes.",
        "The dashboard serves HTTPS with a self-signed certificate on first boot. Upload a real one under Settings → Control plane HTTPS, or set TLS_ENABLED=false for plain HTTP.",
      ],
      screenshot: {
        key: "installCompose",
        alt: "Terminal showing docker compose up -d creating the network and volumes, starting PostgreSQL and PingWAF and printing the dashboard URL",
        caption:
          "docker compose up -d pulls the image, starts PostgreSQL, waits for it to become healthy and brings the dashboard up.",
      },
    },
  },
  interactive: {
    id: "interactive",
    eyebrow: "Interactive setup",
    title: "Or let the installer ask",
    lead: "Run the script from a terminal without flags and it walks through mode, credentials, database and certificate names, then prints the resolved plan for confirmation.",
    steps: [
      {
        title: "Choose the deployment mode",
        body: "1 all-in-one, 2 server or 3 agent — press Enter to accept the bracketed default.",
      },
      {
        title: "Fill in the parameters",
        body: "Agents ask for the control plane URL and enrollment key; server modes ask for the administrator account, PostgreSQL URL and certificate names. Secrets stay hidden while typed.",
      },
      {
        title: "Confirm the summary",
        body: "Before anything is written, the installer prints the resolved plan — mode, version, platform, paths, systemd state and a masked database URL.",
      },
      {
        title: "Finish and verify",
        body: "Server modes print the start command; agents are enabled and started automatically. Confirm the node under Nodes in the dashboard.",
      },
    ],
    terminalNote:
      "Interactive mode needs a local terminal. Under curl | bash stdin is the script itself — download it first, then run bash install.sh to answer the prompts.",
    shots: [
      {
        key: "installInteractive",
        alt: "Interactive installer prompting for mode, administrator account, PostgreSQL URL and certificate names, ending in the installation plan",
        caption:
          "The interactive installer prompts in order and prints the resolved plan before writing anything.",
      },
      {
        key: "installSuccess",
        alt: "Installer success banner listing binary, config, data and service paths plus next steps",
        caption: "A successful run registers pingwaf.service and prints where everything landed.",
      },
    ],
  },
  reference: {
    id: "reference",
    eyebrow: "Flag reference",
    title: "Installer flags at a glance",
    lead: "Every flag install.sh accepts. Each one also has a PINGWAF_* environment variable equivalent.",
    rows: [
      {
        flag: "--mode",
        description:
          "Operating mode: all-in-one, server or agent. Defaults to all-in-one.",
      },
      {
        flag: "--version",
        description:
          "Install a specific release (a leading v is stripped). Empty or 'latest' resolves the newest GitHub release.",
      },
      {
        flag: "--db-url",
        description:
          "PostgreSQL connection string for server/all-in-one. Defaults to postgres://pingwaf:pingwaf@localhost:5432/pingwaf.",
      },
      {
        flag: "--server-url",
        description:
          "Control plane gRPC URL for agent mode. Use https:// — gRPC TLS is served by default.",
      },
      {
        flag: "--api-key",
        description:
          "Agent enrollment token. Can also be supplied as PINGWAF_API_KEY to keep it off the command line.",
      },
      {
        flag: "--admin-email",
        description:
          "Seeded administrator e-mail (server/all-in-one). Defaults to admin@pingwaf.local.",
      },
      {
        flag: "--admin-password",
        description:
          "Seeded administrator password (server/all-in-one). Empty keeps pingwaf123, which the console forces you to change at first sign-in.",
      },
      {
        flag: "--tls-sans",
        description:
          "Comma-separated hostnames the generated dashboard certificate should cover.",
      },
      {
        flag: "--no-systemd",
        description:
          "Skip creating the systemd unit — for containers, upgrades or hand-rolled process managers.",
      },
      {
        flag: "--interactive / --non-interactive",
        description:
          "Force prompting even when flags are given, or never prompt and use the given values plus defaults. Piping without flags stays non-interactive.",
      },
      {
        flag: "--dry-run",
        description:
          "Print the resolved plan — mode, version, platform, paths — and exit without installing.",
      },
    ],
    link: {
      label: "CLI handbook on GitHub",
      path: "https://github.com/shuaiZend/PingWAF/blob/main/docs/cli.md",
    },
  },
  troubleshooting: {
    id: "troubleshooting",
    eyebrow: "Troubleshooting",
    title: "When the install does not come up clean",
    lead: "The four failures seen most often in support, and the command that pinpoints each one.",
    items: [
      {
        q: "The installer cannot reach PostgreSQL",
        a: "all-in-one and server modes need PostgreSQL 14+ before first boot. The script warns when psql cannot connect — install the server, create the pingwaf user and database (createuser / createdb), then re-run the installer or edit /etc/pingwaf/pingwaf.toml and run systemctl restart pingwaf.",
      },
      {
        q: "bind() failed on 0.0.0.0:80 or :443",
        a: "The data plane binds 80/443 as the unprivileged pingwaf user, which only works because the unit grants CAP_NET_BIND_SERVICE. If binding still fails, check systemctl show pingwaf -p AmbientCapabilities and add the capability with a drop-in: sudo systemctl edit pingwaf → [Service] AmbientCapabilities=CAP_NET_BIND_SERVICE. Units written before the capability was granted keep failing until the drop-in is added.",
      },
      {
        q: "The binary fails with a versioned glibc symbol error",
        a: "Release assets are built on Ubuntu 24.04 and need glibc 2.39 or newer. On older distributions (Debian 11, Ubuntu 20.04, CentOS/RHEL, Alpine, …) the installer's smoke test stops with a clear message — build from source instead.",
      },
      {
        q: "The systemd service will not start",
        a: "Inspect the unit log with journalctl -u pingwaf -e and review /etc/pingwaf/pingwaf.toml. There is no reload: apply any edit with systemctl restart pingwaf and check systemctl status pingwaf afterwards.",
      },
    ],
  },
  cta: {
    title: "Installed? Now add your first site.",
    lead: "The docs cover the site model, distributed topologies, configuration precedence and the full environment variable reference.",
    primary: { label: "Read the docs", path: "/docs/" },
    secondary: {
      label: "Releases on GitHub",
      path: "https://github.com/shuaiZend/PingWAF/releases",
    },
  },
} satisfies InstallContent;

export default install;

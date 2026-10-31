import type { InstallContent } from "../schema";

const install = {
  meta: {
    title: "Installation et déploiement — PingWAF",
    description:
      "Une commande installe PingWAF avec Docker Compose : plan de contrôle, agent intégré et PostgreSQL, données et certificats sur des volumes nommés.",
  },
  hero: {
    eyebrow: "Installation",
    title: "De zéro à un nœud opérationnel en une commande",
    lead: "Docker Compose est la méthode recommandée : une commande démarre le plan de contrôle, l'agent intégré et PostgreSQL. L'installation par script reste disponible pour les hôtes gérés par systemd.",
    badges: [
      { icon: "terminal", label: "Exécution", value: "Une commande" },
      { icon: "box", label: "Modes", value: "all-in-one · server · agent" },
      { icon: "shield-check", label: "Plateforme", value: "Linux amd64 / arm64" },
      { icon: "clock", label: "Durée", value: "~2 minutes" },
    ],
    toc: [
      { id: "builder", label: "Générateur de commande" },
      { id: "interactive", label: "Installation interactive" },
      { id: "reference", label: "Référence des flags" },
      { id: "troubleshooting", label: "Dépannage" },
    ],
    oneLiner:
      "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
  },
  builder: {
    id: "builder",
    eyebrow: "Générateur de commande",
    title: "Composez votre commande d'installation",
    lead: "Docker Compose est présélectionné — une commande, base de données incluse. Les champs sont préremplis ; ne modifiez que le nécessaire. Le script reste disponible dans le second onglet.",
    modes: [
      {
        id: "all-in-one",
        label: "All-in-one",
        body: "Plan de contrôle et agent du plan de données dans un seul processus sur cet hôte. Nécessite un PostgreSQL 14+ joignable ; le tableau de bord est servi sur :9080.",
        flags: ["--mode all-in-one", "--db-url postgres://pingwaf:•••@db:5432/pingwaf"],
      },
      {
        id: "server",
        label: "Server",
        body: "Plan de contrôle seul — tableau de bord, API et gRPC sur :9080/:9090. À choisir lorsque les agents de bord tournent sur des hôtes séparés.",
        flags: ["--mode server", "--db-url postgres://…"],
      },
      {
        id: "agent",
        label: "Agent",
        body: "Un nœud du plan de données qui proxifie le trafic et se connecte au plan de contrôle via gRPC. Ne conserve aucune base de données ; requiert --server-url et une clé d'enrôlement.",
        flags: ["--mode agent", "--server-url https://cp:9090", "--api-key pwk_…"],
      },
    ],
    fields: [
      {
        key: "version",
        label: "Version",
        placeholder: "ex. 0.33.0 (vide = dernière version)",
        hint: "Épinglez une version sans le v initial. 'latest' (la valeur par défaut préremplie) ou vide installe la dernière publication.",
        modes: ["all-in-one", "server", "agent"],
        optional: true,
      },
      {
        key: "dbUrl",
        label: "URL PostgreSQL",
        placeholder: "postgres://pingwaf:pingwaf@localhost:5432/pingwaf",
        hint: "Chaîne de connexion de la base du plan de contrôle. Le schéma est créé au premier démarrage.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "serverUrl",
        label: "URL du plan de contrôle",
        placeholder: "https://control-plane.example.com:9090",
        hint: "Point de terminaison gRPC du plan de contrôle. Utilisez https:// — le TLS gRPC est actif par défaut ; téléchargez le CA sous Paramètres → Control plane HTTPS.",
        modes: ["agent"],
      },
      {
        key: "apiKey",
        label: "Clé API d'enrôlement",
        placeholder: "pwk_…",
        hint: "Jeton d'enrôlement de l'agent émis par le plan de contrôle. Préférez la variable d'environnement PINGWAF_API_KEY pour éviter l'historique du shell.",
        modes: ["agent"],
        secret: true,
      },
      {
        key: "adminEmail",
        label: "E-mail de l'administrateur",
        placeholder: "ops@example.com",
        hint: "Compte administrateur initialisé. Par défaut admin@pingwaf.local si laissé vide.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "adminPassword",
        label: "Mot de passe administrateur",
        placeholder: "••••••••",
        hint: "Optionnel — laissez vide pour conserver le mot de passe documenté (pingwaf123) ; la console impose de le changer à la première connexion. C'est pourquoi le générateur ne l'écrit jamais dans la commande.",
        modes: ["all-in-one", "server"],
        optional: true,
        secret: true,
      },
      {
        key: "tlsSans",
        label: "Noms du certificat (SAN)",
        placeholder: "waf.example.com, waf.internal",
        hint: "Noms d'hôtes séparés par des virgules que le certificat généré du tableau de bord doit couvrir. Vide couvre le nom que vous utilisez pour y accéder.",
        modes: ["all-in-one", "server"],
        optional: true,
      },
    ],
    optionalLabel: "optionnel",
    defaultsLabel:
      "Prérempli avec les valeurs par défaut de l'installateur — seules les valeurs que vous modifiez sont ajoutées à la commande.",
    resetLabel: "Rétablir les valeurs par défaut",
    commandTitle: "Votre commande d'installation",
    note: "Seules les valeurs différentes des valeurs par défaut sont ajoutées comme flags : le générateur intact produit donc la ligne la plus courte. Les arguments atteignent le script canalisé via bash -s --.",
    secretWarning:
      "La clé API apparaît en clair dans la commande et peut finir dans l'historique du shell. Préférez exporter PINGWAF_API_KEY, ou exécutez l'installateur en mode interactif et collez la clé lorsqu'elle est demandée.",
    copy: "Copier",
    copied: "Copié",
    emptyHint:
      "Le mode agent a encore besoin d'une URL de plan de contrôle et d'une clé d'enrôlement — la commande est générée, mais l'agent ne pourra pas s'enregistrer tant que les deux ne sont pas fournis.",
    scriptUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh",
  },
  deployment: {
    tabs: { compose: "Docker Compose", script: "Script d'installation" },
    compose: {
      tabLabel: "Docker Compose",
      recommendedBadge: "Recommandé",
      installerUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh",
      oneLiner: "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash",
      oneLinerTitle: "Installation en une ligne",
      oneLinerNote: "Lancez-la dans un répertoire vide : l'installateur télécharge docker-compose.yml, écrit un .env privé, récupère l'image et démarre la stack. Un .env existant n'est jamais écrasé — relancez la même commande pour mettre à jour.",
      customizeTitle: "Besoin de personnaliser ? Générez une commande avec options",
      customizeHint: "Ne modifiez que le nécessaire ; la commande n'ajoute que les options différentes des valeurs par défaut. Le mot de passe administrateur n'est jamais écrit dans la commande.",
      title: "Déployez avec Docker Compose",
      lead: "Docker Compose est la méthode recommandée : une commande démarre le plan de contrôle, son agent intégré et PostgreSQL, avec données et certificats dans des volumes nommés. Une mise à jour ne change que le tag de l'image.",
      steps: [
        {
          title: "Lancez la commande",
          body: "Exécutez la commande ci-dessus dans un répertoire vide. L'installateur crée ./pingwaf, télécharge le compose et écrit un .env privé.",
        },
        {
          title: "Attendez la stack",
          body: "Compose récupère l'image, démarre PostgreSQL, attend son health check puis lance PingWAF. Le premier démarrage prend une à deux minutes.",
        },
        {
          title: "Connectez-vous",
          body: "Ouvrez https://<hôte>:9080 et utilisez ADMIN_EMAIL / ADMIN_PASSWORD (le mot de passe par défaut doit être changé à la première connexion), puis ajoutez un site.",
        }
      ],
      fields: [
        {
          key: "tag",
          label: "Tag de l'image",
          placeholder: "latest",
          hint: "Tag de l'image ghcr.io/shuaizend/pingwaf. 'latest' suit la dernière publication ; épinglez une version publiée (ex. 0.33.0) en production.",
          defaultValue: "latest",
        },
        {
          key: "adminEmail",
          label: "E-mail de l'administrateur",
          placeholder: "admin@pingwaf.local",
          hint: "Compte administrateur initialisé. Par défaut : admin@pingwaf.local.",
          defaultValue: "admin@pingwaf.local",
        },
        {
          key: "tlsSans",
          label: "Noms du certificat (SAN)",
          placeholder: "waf.example.com",
          hint: "Noms d'hôtes séparés par des virgules que le certificat auto-signé du tableau de bord doit couvrir. Vide couvre le nom que vous utilisez pour y accéder.",
          defaultValue: "",
        },
        {
          key: "httpPort",
          label: "Port HTTP (hôte)",
          placeholder: "80",
          hint: "Port hôte pour le trafic HTTP proxifié. Modifiez-le si le 80 est déjà occupé ; le port dans le conteneur reste 80.",
          defaultValue: "80",
        },
        {
          key: "httpsPort",
          label: "Port HTTPS (hôte)",
          placeholder: "443",
          hint: "Port hôte pour le trafic HTTPS proxifié. Modifiez-le si le 443 est déjà occupé ; le port dans le conteneur reste 443.",
          defaultValue: "443",
        },
        {
          key: "adminPort",
          label: "Port du tableau de bord (hôte)",
          placeholder: "9080",
          hint: "Port hôte pour le tableau de bord et l'API REST. Le gRPC des agents reste sur :9090.",
          defaultValue: "9080",
        },
      ],
      envTitle: ".env généré",
      commandTitle: "Démarrer la pile",
      note: "Première connexion : https://<votre-hôte>:9080 avec le ADMIN_EMAIL / ADMIN_PASSWORD ci-dessus (acceptez l'avertissement du certificat auto-signé jusqu'à en téléverser un vrai). Les agents joignent le plan de contrôle sur :9090.",
      upgradeTitle: "Mise à niveau",
      upgradeBody:
        "Télécharge l'image du tag de votre .env et recrée le conteneur avec les mêmes volumes — configuration, données et certificats survivent au redémarrage. Épinglez le tag pour des mises à niveau maîtrisées.",
      upgradeCommand: "docker compose pull && docker compose up -d",
      requirements: [
        "Docker Engine 24+ avec le plugin Compose v2 (docker compose, pas l'ancien binaire docker-compose).",
        "Ports 80, 443, 9080 et 9090 libres sur l'hôte — ou remplacez-les dans les champs ci-dessous.",
        "Aucune base de données externe requise : le fichier compose embarque son propre conteneur PostgreSQL 16 avec des volumes nommés.",
        "Le tableau de bord sert HTTPS avec un certificat auto-signé au premier démarrage. Téléversez-en un vrai sous Paramètres → Control plane HTTPS, ou mettez TLS_ENABLED=false pour du HTTP simple.",
      ],
      screenshot: {
        key: "installCompose",
        alt: "Terminal montrant docker compose up -d créant le réseau et les volumes, démarrant PostgreSQL et PingWAF puis affichant l'URL du tableau de bord",
        caption:
          "docker compose up -d télécharge l'image, démarre PostgreSQL, attend qu'il soit healthy et lance le tableau de bord.",
      },
    },
  },
  interactive: {
    id: "interactive",
    eyebrow: "Installation interactive",
    title: "Ou laissez l'installateur poser les questions",
    lead: "Lancez le script dans un terminal sans flags : il passe en revue le mode, les identifiants, la base de données et les noms de certificat, puis affiche le plan résolu pour confirmation.",
    steps: [
      {
        title: "Choisissez le mode de déploiement",
        body: "1 all-in-one, 2 server ou 3 agent — appuyez sur Entrée pour accepter la valeur par défaut entre crochets.",
      },
      {
        title: "Renseignez les paramètres",
        body: "Les agents demandent l'URL du plan de contrôle et la clé d'enrôlement ; les modes server demandent le compte administrateur, l'URL PostgreSQL et les noms de certificat. Les secrets restent masqués pendant la saisie.",
      },
      {
        title: "Confirmez le résumé",
        body: "Avant d'écrire quoi que ce soit, l'installateur affiche le plan résolu — mode, version, plateforme, chemins, état systemd et URL de base de données masquée.",
      },
      {
        title: "Terminez et vérifiez",
        body: "Les modes server affichent la commande de démarrage ; les agents sont activés et démarrés automatiquement. Vérifiez le nœud sous Nodes dans le tableau de bord.",
      },
    ],
    terminalNote:
      "Le mode interactif nécessite un terminal local. Avec curl | bash, stdin est le script lui-même — téléchargez-le d'abord, puis lancez bash install.sh pour répondre aux questions.",
    shots: [
      {
        key: "installInteractive",
        alt: "Installateur interactif demandant le mode, le compte administrateur, l'URL PostgreSQL et les noms de certificat, puis affichant le plan d'installation",
        caption:
          "L'installateur interactif pose les questions dans l'ordre et affiche le plan résolu avant d'écrire quoi que ce soit.",
      },
      {
        key: "installSuccess",
        alt: "Bannière de succès de l'installateur listant les chemins du binaire, de la configuration, des données et du service, plus les étapes suivantes",
        caption:
          "Une exécution réussie enregistre pingwaf.service et affiche où tout a été déposé.",
      },
    ],
  },
  reference: {
    id: "reference",
    eyebrow: "Référence des flags",
    title: "Les flags de l'installateur en un coup d'œil",
    lead: "Tous les flags acceptés par install.sh. Chacun possède un équivalent sous forme de variable d'environnement PINGWAF_*.",
    rows: [
      {
        flag: "--mode",
        description:
          "Mode de fonctionnement : all-in-one, server ou agent. Par défaut all-in-one.",
      },
      {
        flag: "--version",
        description:
          "Installe une version précise (le v initial est retiré). Vide ou 'latest' résout la dernière publication GitHub.",
      },
      {
        flag: "--db-url",
        description:
          "Chaîne de connexion PostgreSQL pour server/all-in-one. Par défaut postgres://pingwaf:pingwaf@localhost:5432/pingwaf.",
      },
      {
        flag: "--server-url",
        description:
          "URL gRPC du plan de contrôle pour le mode agent. Utilisez https:// — gRPC est servi en TLS par défaut.",
      },
      {
        flag: "--api-key",
        description:
          "Jeton d'enrôlement de l'agent. Peut aussi être fourni via PINGWAF_API_KEY pour ne pas figurer sur la ligne de commande.",
      },
      {
        flag: "--admin-email",
        description:
          "E-mail de l'administrateur initialisé (server/all-in-one). Par défaut admin@pingwaf.local.",
      },
      {
        flag: "--admin-password",
        description:
          "Mot de passe de l'administrateur initialisé (server/all-in-one). Vide conserve pingwaf123, que la console impose de changer à la première connexion.",
      },
      {
        flag: "--tls-sans",
        description:
          "Noms d'hôtes séparés par des virgules que le certificat généré du tableau de bord doit couvrir.",
      },
      {
        flag: "--no-systemd",
        description:
          "Ne pas créer l'unité systemd — pour les conteneurs, les mises à niveau ou vos propres superviseurs de processus.",
      },
      {
        flag: "--interactive / --non-interactive",
        description:
          "Force les questions même lorsque des flags sont fournis, ou ne pose jamais de question et utilise les valeurs fournies plus les défauts. Canalisé sans flags, le script reste non interactif.",
      },
      {
        flag: "--dry-run",
        description:
          "Affiche le plan résolu — mode, version, plateforme, chemins — puis quitte sans installer.",
      },
    ],
    link: {
      label: "Manuel CLI sur GitHub",
      path: "https://github.com/shuaiZend/PingWAF/blob/main/docs/cli.md",
    },
  },
  troubleshooting: {
    id: "troubleshooting",
    eyebrow: "Dépannage",
    title: "Quand l'installation ne se termine pas proprement",
    lead: "Les quatre échecs les plus fréquents au support, et la commande qui permet de cibler chacun.",
    items: [
      {
        q: "L'installateur ne peut pas joindre PostgreSQL",
        a: "Les modes all-in-one et server exigent PostgreSQL 14+ avant le premier démarrage. Le script avertit lorsque psql ne peut pas se connecter — installez le serveur, créez l'utilisateur et la base pingwaf (createuser / createdb), relancez l'installateur ou modifiez /etc/pingwaf/pingwaf.toml puis exécutez systemctl restart pingwaf.",
      },
      {
        q: "bind() failed sur 0.0.0.0:80 ou :443",
        a: "Le plan de données lie 80/443 sous l'utilisateur non privilégié pingwaf, ce qui ne fonctionne que parce que l'unité accorde CAP_NET_BIND_SERVICE. Si la liaison échoue encore, vérifiez systemctl show pingwaf -p AmbientCapabilities et ajoutez la capacité via un drop-in : sudo systemctl edit pingwaf → [Service] AmbientCapabilities=CAP_NET_BIND_SERVICE. Les unités écrites avant l'octroi de la capacité continuent d'échouer tant que le drop-in n'est pas ajouté.",
      },
      {
        q: "Le binaire échoue avec une erreur de symbole glibc versionné",
        a: "Les artefacts de publication sont compilés sur Ubuntu 24.04 et exigent glibc 2.39 ou plus récent. Sur des distributions plus anciennes (Debian 11, Ubuntu 20.04, CentOS/RHEL, Alpine, …), le test de fumée de l'installateur s'arrête avec un message clair — compilez depuis les sources à la place.",
      },
      {
        q: "Le service systemd ne démarre pas",
        a: "Inspectez le journal de l'unité avec journalctl -u pingwaf -e et vérifiez /etc/pingwaf/pingwaf.toml. Il n'y a pas de rechargement : appliquez toute modification avec systemctl restart pingwaf puis contrôlez avec systemctl status pingwaf.",
      },
    ],
  },
  cta: {
    title: "Installé ? Ajoutez maintenant votre premier site.",
    lead: "La documentation couvre le modèle de site, les topologies distribuées, la priorité de configuration et la référence complète des variables d'environnement.",
    primary: { label: "Lire la documentation", path: "/docs/" },
    secondary: {
      label: "Releases sur GitHub",
      path: "https://github.com/shuaiZend/PingWAF/releases",
    },
  },
} satisfies InstallContent;

export default install;

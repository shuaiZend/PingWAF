import type { InstallContent } from "../schema";

const install = {
  meta: {
    title: "安装部署 — PingWAF",
    description:
      "一条命令在任意 Linux 服务器上安装 PingWAF：选择 all-in-one、server 或 agent 模式，传参或使用交互式引导，自动注册加固过的 systemd 服务。",
  },
  hero: {
    eyebrow: "安装部署",
    title: "一条命令，从零到一个运行中的节点",
    lead: "官方安装脚本自动识别平台、下载对应二进制、写入 /etc/pingwaf/pingwaf.toml 并注册加固过的 systemd 服务。在下方拼装命令，或直接使用交互式引导。",
    badges: [
      { icon: "terminal", label: "运行", value: "一条命令" },
      { icon: "box", label: "模式", value: "all-in-one · server · agent" },
      { icon: "shield-check", label: "平台", value: "Linux amd64 / arm64" },
      { icon: "clock", label: "耗时", value: "约 2 分钟" },
    ],
    toc: [
      { id: "builder", label: "命令生成器" },
      { id: "interactive", label: "交互式安装" },
      { id: "reference", label: "参数速查" },
      { id: "troubleshooting", label: "故障排查" },
    ],
    oneLiner:
      "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash",
  },
  builder: {
    id: "builder",
    eyebrow: "命令生成器",
    title: "拼装你的安装命令",
    lead: "选择推荐的 Docker Compose 部署，或一键脚本安装。各字段已预填文档默认值，只需修改需要自定义的部分；所有内容均在本地生成，不会发送到任何其他地方。",
    modes: [
      {
        id: "all-in-one",
        label: "All-in-one",
        body: "控制面与数据面 Agent 在同一进程运行。需要可连通的 PostgreSQL 14+；控制台在 :9080 提供服务。",
        flags: ["--mode all-in-one", "--db-url postgres://pingwaf:•••@db:5432/pingwaf"],
      },
      {
        id: "server",
        label: "Server",
        body: "只运行控制面——控制台、API 与 gRPC 分别在 :9080/:9090。当边缘 Agent 部署在其他主机时选择此模式。",
        flags: ["--mode server", "--db-url postgres://…"],
      },
      {
        id: "agent",
        label: "Agent",
        body: "代理流量的数据面节点，通过 gRPC 连接控制面。不保留数据库；需要 --server-url 与注册密钥。",
        flags: ["--mode agent", "--server-url https://cp:9090", "--api-key pwk_…"],
      },
    ],
    fields: [
      {
        key: "version",
        label: "版本",
        placeholder: "如 0.33.0（留空 = 最新版）",
        hint: "填入不带 v 前缀的发布号。'latest'（预填默认值）或留空均表示安装最新发布的版本。",
        modes: ["all-in-one", "server", "agent"],
        optional: true,
      },
      {
        key: "dbUrl",
        label: "PostgreSQL 连接串",
        placeholder: "postgres://pingwaf:pingwaf@localhost:5432/pingwaf",
        hint: "控制面数据库的连接串。schema 会在首次启动时自动创建。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "serverUrl",
        label: "控制面地址",
        placeholder: "https://control-plane.example.com:9090",
        hint: "控制面的 gRPC 端点。请使用 https://——gRPC TLS 默认开启；可从控制台「设置 → Control plane HTTPS」下载 CA 证书。",
        modes: ["agent"],
      },
      {
        key: "apiKey",
        label: "注册 API Key",
        placeholder: "pwk_…",
        hint: "控制台下发的 Agent 注册令牌。建议改用 PINGWAF_API_KEY 环境变量传入，避免进入 shell 历史。",
        modes: ["agent"],
        secret: true,
      },
      {
        key: "adminEmail",
        label: "管理员邮箱",
        placeholder: "ops@example.com",
        hint: "初始化的管理员账号。留空时默认为 admin@pingwaf.local。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "adminPassword",
        label: "管理员密码",
        placeholder: "••••••••",
        hint: "可选——留空即使用文档默认密码（pingwaf123），控制台会在首次登录时强制修改。因此生成器不会把密码写入命令。",
        modes: ["all-in-one", "server"],
        optional: true,
        secret: true,
      },
      {
        key: "tlsSans",
        label: "证书名称（SAN）",
        placeholder: "waf.example.com, waf.internal",
        hint: "逗号分隔的主机名，生成的控制台证书会覆盖这些名称。留空则覆盖访问控制台所用的主机名。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
    ],
    optionalLabel: "可选",
    defaultsLabel: "已预填安装器默认值——只有你修改过的值才会写进命令。",
    resetLabel: "恢复默认",
    commandTitle: "生成的安装命令",
    note: "只有与默认值不同的项才会以参数形式追加，因此不做任何修改时生成的就是最短的一行命令。参数通过 bash -s -- 传给管道中的脚本。",
    secretWarning:
      "API Key 会明文出现在命令中，可能进入 shell 历史。推荐改用 export PINGWAF_API_KEY=… 传入，或交互运行安装器、在提问时粘贴密钥。",
    copy: "复制",
    copied: "已复制",
    emptyHint:
      "Agent 模式还需要控制面地址与注册密钥——命令仍然会生成，但在两者补齐之前 Agent 无法注册。",
    scriptUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh",
  },
  deployment: {
    tabs: { compose: "Docker Compose", script: "脚本安装" },
    compose: {
      tabLabel: "Docker Compose",
      recommendedBadge: "推荐",
      title: "使用 Docker Compose 部署",
      lead: "一份 compose 文件即可拉起 PingWAF（all-in-one）及其自带的 PostgreSQL 数据库，数据与证书均使用命名卷持久化。把生成的 .env 保存到 docker-compose.yml 旁边，然后启动即可。",
      steps: [
        {
          title: "准备文件",
          body: "将 docker-compose.yml 放入一个空目录，并把生成的 .env 保存到旁边。默认值即可直接使用——只设置需要自定义的项。",
        },
        {
          title: "启动",
          body: "执行 docker compose up -d。Compose 会拉取指定镜像、启动 PostgreSQL 并等待其健康检查通过，然后按 .env 中的账号与端口启动 PingWAF。",
        },
        {
          title: "登录并添加站点",
          body: "在管理端口（默认 :9080）打开控制台，用 ADMIN_EMAIL / ADMIN_PASSWORD 登录。若沿用默认密码，控制台会在首次登录时强制修改。",
        },
      ],
      fields: [
        {
          key: "tag",
          label: "镜像标签",
          placeholder: "latest",
          hint: "ghcr.io/shuaizend/pingwaf 的镜像标签。'latest' 跟随最新发布；生产环境建议固定某个发布版本（如 0.33.0）。",
          defaultValue: "latest",
        },
        {
          key: "adminEmail",
          label: "管理员邮箱",
          placeholder: "admin@pingwaf.local",
          hint: "初始化的管理员账号。默认 admin@pingwaf.local。",
          defaultValue: "admin@pingwaf.local",
        },
        {
          key: "adminPassword",
          label: "管理员密码",
          placeholder: "pingwaf123",
          hint: "写入 .env 文件——不会进入 shell 历史。文档默认密码为 pingwaf123，控制台会在首次登录时强制修改。",
          defaultValue: "pingwaf123",
        },
        {
          key: "tlsSans",
          label: "证书名称（SAN）",
          placeholder: "waf.example.com",
          hint: "逗号分隔的主机名，自签名的控制台证书会覆盖这些名称。留空则覆盖访问控制台所用的主机名。",
          defaultValue: "",
        },
        {
          key: "httpPort",
          label: "HTTP 端口（宿主机）",
          placeholder: "80",
          hint: "代理 HTTP 流量的宿主机端口。80 被占用时可改；容器内端口保持 80。",
          defaultValue: "80",
        },
        {
          key: "httpsPort",
          label: "HTTPS 端口（宿主机）",
          placeholder: "443",
          hint: "代理 HTTPS 流量的宿主机端口。443 被占用时可改；容器内端口保持 443。",
          defaultValue: "443",
        },
        {
          key: "adminPort",
          label: "控制台端口（宿主机）",
          placeholder: "9080",
          hint: "控制台与 REST API 的宿主机端口。Agent 的 gRPC 仍在 :9090。",
          defaultValue: "9080",
        },
      ],
      envTitle: "生成的 .env",
      commandTitle: "启动命令",
      note: "首次登录：https://<你的主机>:9080，使用上方的 ADMIN_EMAIL / ADMIN_PASSWORD（上传正式证书前请先接受自签名证书警告）。Agent 通过 :9090 连接控制面。",
      upgradeTitle: "升级",
      upgradeBody:
        "拉取 .env 中指定标签的镜像并按原有卷重建容器——配置、数据与证书在重启后全部保留。需要可控升级时请固定镜像标签。",
      upgradeCommand: "docker compose pull && docker compose up -d",
      requirements: [
        "Docker Engine 24+，并安装 Compose v2 插件（docker compose，而非旧的 docker-compose 命令）。",
        "宿主机 80、443、9080、9090 端口空闲——或在下方字段中改用其他端口。",
        "无需外部数据库：compose 文件自带 PostgreSQL 16 容器，并使用命名卷持久化。",
        "控制台首次启动以自签名证书提供 HTTPS。可在「设置 → Control plane HTTPS」上传正式证书，或设置 TLS_ENABLED=false 改用纯 HTTP。",
      ],
      screenshot: {
        key: "installCompose",
        alt: "终端显示 docker compose up -d 创建网络与卷、启动 PostgreSQL 与 PingWAF 并打印控制台地址",
        caption: "docker compose up -d 拉取镜像、启动 PostgreSQL，等待健康检查通过后拉起控制台。",
      },
    },
  },
  interactive: {
    id: "interactive",
    eyebrow: "交互式安装",
    title: "或者让安装器来提问",
    lead: "在终端不带参数运行脚本，它会依次询问模式、账号、数据库与证书名称，打印解析后的计划并等待确认。",
    steps: [
      {
        title: "选择部署模式",
        body: "1 all-in-one、2 server 或 3 agent——直接回车接受方括号中的默认值。",
      },
      {
        title: "填写参数",
        body: "Agent 会询问控制面地址与注册密钥；server 模式会询问管理员账号、PostgreSQL 连接串与证书名称。密码输入时不会回显。",
      },
      {
        title: "确认摘要",
        body: "在写入任何内容之前，安装器会先打印解析后的计划——模式、版本、平台、路径、systemd 状态与脱敏后的数据库 URL。",
      },
      {
        title: "完成并验证",
        body: "server 模式会打印启动命令；agent 会被自动启用并启动。到控制台的「节点」页面确认节点已上线。",
      },
    ],
    terminalNote:
      "交互模式需要本地终端。curl | bash 场景下 stdin 是脚本本身——请先下载脚本，再用 bash install.sh 运行以进入交互引导。",
    shots: [
      {
        key: "installInteractive",
        alt: "交互式安装器依次询问模式、管理员账号、PostgreSQL 连接串与证书名称，最后打印安装计划",
        caption: "交互式安装器按顺序提问，并在写入任何内容前打印解析后的计划。",
      },
      {
        key: "installSuccess",
        alt: "安装成功的横幅，列出二进制、配置、数据与服务路径以及后续步骤",
        caption: "安装成功后会注册 pingwaf.service，并打印各文件的落点。",
      },
    ],
  },
  reference: {
    id: "reference",
    eyebrow: "参数速查",
    title: "安装器参数一览",
    lead: "install.sh 支持的全部参数，每个参数都有对应的 PINGWAF_* 环境变量。",
    rows: [
      {
        flag: "--mode",
        description: "运行模式：all-in-one、server 或 agent。默认 all-in-one。",
      },
      {
        flag: "--version",
        description: "安装指定版本（自动去掉前导 v）。留空或填 latest 则解析最新的 GitHub 发布。",
      },
      {
        flag: "--db-url",
        description:
          "server/all-in-one 模式的 PostgreSQL 连接串。默认 postgres://pingwaf:pingwaf@localhost:5432/pingwaf。",
      },
      {
        flag: "--server-url",
        description: "agent 模式的控制面 gRPC 地址。请使用 https://——gRPC 默认以 TLS 提供服务。",
      },
      {
        flag: "--api-key",
        description: "Agent 注册令牌。也可通过 PINGWAF_API_KEY 传入，避免留在命令行上。",
      },
      {
        flag: "--admin-email",
        description: "初始化的管理员邮箱（server/all-in-one）。默认 admin@pingwaf.local。",
      },
      {
        flag: "--admin-password",
        description:
          "初始化的管理员密码（server/all-in-one）。留空保留 pingwaf123，控制台会在首次登录时强制修改。",
      },
      {
        flag: "--tls-sans",
        description: "逗号分隔的主机名，生成的控制台证书会覆盖这些名称。",
      },
      {
        flag: "--no-systemd",
        description: "跳过创建 systemd 服务——适用于容器、升级或自行管理进程的场景。",
      },
      {
        flag: "--interactive / --non-interactive",
        description:
          "强制进入提问（即使已传参），或永不提问、直接使用给定值与默认值。管道方式且不带参数时保持非交互。",
      },
      {
        flag: "--dry-run",
        description: "只打印解析后的计划——模式、版本、平台、路径——然后退出，不做任何安装。",
      },
    ],
    link: {
      label: "GitHub 上的 CLI 手册",
      path: "https://github.com/shuaiZend/PingWAF/blob/main/docs/cli.md",
    },
  },
  troubleshooting: {
    id: "troubleshooting",
    eyebrow: "故障排查",
    title: "安装没有顺利收尾时",
    lead: "支持中最常见的四类失败，以及定位各自问题所需的命令。",
    items: [
      {
        q: "安装器连不上 PostgreSQL",
        a: "all-in-one 与 server 模式在首次启动前需要 PostgreSQL 14+。脚本在 psql 无法连接时会给出警告——请先安装数据库，创建 pingwaf 用户与数据库（createuser / createdb），然后重新运行安装器，或编辑 /etc/pingwaf/pingwaf.toml 后执行 systemctl restart pingwaf。",
      },
      {
        q: "bind() failed on 0.0.0.0:80 或 :443",
        a: "数据面以非特权用户 pingwaf 绑定 80/443，这依赖服务单元授予的 CAP_NET_BIND_SERVICE。如果绑定仍然失败，先执行 systemctl show pingwaf -p AmbientCapabilities 检查，再用 drop-in 补上：sudo systemctl edit pingwaf → [Service] AmbientCapabilities=CAP_NET_BIND_SERVICE。在授予该能力之前写入的旧单元，会一直失败直到补上 drop-in。",
      },
      {
        q: "二进制报版本化 glibc 符号错误",
        a: "发布产物在 Ubuntu 24.04 上构建，需要 glibc 2.39 或更新。在更老的发行版（Debian 11、Ubuntu 20.04、CentOS/RHEL、Alpine 等）上，安装器的冒烟测试会报错停止——请改为源码构建。",
      },
      {
        q: "systemd 服务启动失败",
        a: "用 journalctl -u pingwaf -e 查看单元日志，并检查 /etc/pingwaf/pingwaf.toml。没有 reload 机制：任何修改都通过 systemctl restart pingwaf 应用，之后用 systemctl status pingwaf 确认。",
      },
    ],
  },
  cta: {
    title: "装好了？接下来添加第一个站点。",
    lead: "文档会带你了解站点模型、分布式拓扑、配置优先级与完整的环境变量参考。",
    primary: { label: "阅读文档", path: "/docs/" },
    secondary: {
      label: "GitHub Releases",
      path: "https://github.com/shuaiZend/PingWAF/releases",
    },
  },
} satisfies InstallContent;

export default install;

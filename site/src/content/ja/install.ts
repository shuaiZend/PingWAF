import type { InstallContent } from "../schema";

const install = {
  meta: {
    title: "インストールとデプロイ — PingWAF",
    description:
      "1 つのコマンドで任意の Linux サーバーに PingWAF を導入：all-in-one・server・agent の各モードを選び、フラグを渡すか対話式インストーラーに答えるだけで、強化済みの systemd サービスが手に入ります。",
  },
  hero: {
    eyebrow: "インストール",
    title: "1 コマンドでゼロから稼働ノードまで",
    lead: "公式インストーラーはプラットフォームを検出し、対応するバイナリをダウンロードし、/etc/pingwaf/pingwaf.toml を書き出して、強化済みの systemd ユニットを登録します。以下でコマンドを組み立てるか、対話式の質問に答えて進めてください。",
    badges: [
      { icon: "terminal", label: "実行", value: "1 コマンド" },
      { icon: "box", label: "モード", value: "all-in-one · server · agent" },
      { icon: "shield-check", label: "プラットフォーム", value: "Linux amd64 / arm64" },
      { icon: "clock", label: "所要時間", value: "約 2 分" },
    ],
    toc: [
      { id: "builder", label: "コマンドビルダー" },
      { id: "interactive", label: "対話式インストール" },
      { id: "reference", label: "フラグ早見表" },
      { id: "troubleshooting", label: "トラブルシューティング" },
    ],
    oneLiner:
      "curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash",
  },
  builder: {
    id: "builder",
    eyebrow: "コマンドビルダー",
    title: "インストールコマンドを組み立てる",
    lead: "推奨の Docker Compose デプロイか、ワンラインのインストールスクリプトを選べます。各フィールドにはドキュメント既定値が事前入力されているので、変更したい部分だけ編集してください。すべてローカルで生成され、他のどこにも送信されません。",
    modes: [
      {
        id: "all-in-one",
        label: "All-in-one",
        body: "コントロールプレーンとデータプレーン Agent を 1 つのプロセスでこのホスト上に実行します。到達可能な PostgreSQL 14+ が必要で、ダッシュボードは :9080 で提供されます。",
        flags: ["--mode all-in-one", "--db-url postgres://pingwaf:•••@db:5432/pingwaf"],
      },
      {
        id: "server",
        label: "Server",
        body: "コントロールプレーンのみ — ダッシュボード・API・gRPC が :9080/:9090 で待ち受けます。エッジ Agent を別ホストで動かす場合はこちらを選びます。",
        flags: ["--mode server", "--db-url postgres://…"],
      },
      {
        id: "agent",
        label: "Agent",
        body: "トラフィックをプロキシし、gRPC でコントロールプレーンに接続するデータプレーンノードです。データベースを持ちません。--server-url と登録キーが必要です。",
        flags: ["--mode agent", "--server-url https://cp:9090", "--api-key pwk_…"],
      },
    ],
    fields: [
      {
        key: "version",
        label: "バージョン",
        placeholder: "例: 0.33.0（空欄 = 最新リリース）",
        hint: "先頭の v を付けずにリリース番号を指定します。'latest'（既定で事前入力済み）または空欄なら最新の公開リリースをインストールします。",
        modes: ["all-in-one", "server", "agent"],
        optional: true,
      },
      {
        key: "dbUrl",
        label: "PostgreSQL URL",
        placeholder: "postgres://pingwaf:pingwaf@localhost:5432/pingwaf",
        hint: "コントロールプレーンのデータベース接続文字列。スキーマは初回起動時に作成されます。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "serverUrl",
        label: "コントロールプレーン URL",
        placeholder: "https://control-plane.example.com:9090",
        hint: "コントロールプレーンの gRPC エンドポイント。https:// を使ってください — gRPC TLS は既定で有効です。CA 証明書は「設定 → Control plane HTTPS」からダウンロードできます。",
        modes: ["agent"],
      },
      {
        key: "apiKey",
        label: "登録用 API キー",
        placeholder: "pwk_…",
        hint: "コントロールプレーンが発行する Agent 登録トークン。シェル履歴に残らないよう PINGWAF_API_KEY 環境変数の利用を推奨します。",
        modes: ["agent"],
        secret: true,
      },
      {
        key: "adminEmail",
        label: "管理者メールアドレス",
        placeholder: "ops@example.com",
        hint: "初期投入される管理者アカウント。空欄の場合は admin@pingwaf.local が使われます。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
      {
        key: "adminPassword",
        label: "管理者パスワード",
        placeholder: "••••••••",
        hint: "任意 — 空欄ならドキュメント既定のパスワード（pingwaf123）が使われ、コンソールが初回ログイン時に変更を強制します。そのためビルダーはコマンドにパスワードを書き込みません。",
        modes: ["all-in-one", "server"],
        optional: true,
        secret: true,
      },
      {
        key: "tlsSans",
        label: "証明書の名称（SAN）",
        placeholder: "waf.example.com, waf.internal",
        hint: "生成されるダッシュボード証明書がカバーするホスト名をカンマ区切りで指定します。空欄ならコンソールへのアクセスに使う名前がカバーされます。",
        modes: ["all-in-one", "server"],
        optional: true,
      },
    ],
    optionalLabel: "任意",
    defaultsLabel:
      "インストーラーの既定値で事前入力済み — 変更した値だけがコマンドに追加されます。",
    resetLabel: "既定値に戻す",
    commandTitle: "生成されたインストールコマンド",
    note: "既定値と異なる値だけがフラグとして追加されるため、未編集のままでは最短のワンライナーが生成されます。引数は bash -s -- でパイプ先のスクリプトに渡されます。",
    secretWarning:
      "API キーはコマンド中に平文で現れ、シェル履歴に残る可能性があります。代わりに PINGWAF_API_KEY をエクスポートするか、インストーラーを対話実行して尋ねられたときに貼り付けてください。",
    copy: "コピー",
    copied: "コピーしました",
    emptyHint:
      "agent モードにはコントロールプレーン URL と登録キーがまだ必要です — コマンドは生成されますが、両方が揃うまで Agent は登録できません。",
    scriptUrl: "https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh",
  },
  deployment: {
    tabs: { compose: "Docker Compose", script: "スクリプトインストール" },
    compose: {
      tabLabel: "Docker Compose",
      recommendedBadge: "推奨",
      title: "Docker Compose でデプロイ",
      lead: "1 つの compose ファイルで PingWAF（all-in-one）と専用の PostgreSQL データベースを起動します。データと証明書は名前付きボリュームに永続化されます。生成された .env を docker-compose.yml の隣に保存して起動してください。",
      steps: [
        {
          title: "ファイルを準備する",
          body: "空のディレクトリに docker-compose.yml を置き、生成された .env を隣に保存します。既定値のままで動作します — 変更したい項目だけ設定してください。",
        },
        {
          title: "スタックを起動する",
          body: "docker compose up -d を実行します。Compose が指定イメージを取得し、PostgreSQL を起動して healthy になるまで待ち、その後 .env の認証情報とポートで PingWAF を起動します。",
        },
        {
          title: "ログインしてサイトを追加する",
          body: "管理ポート（既定 :9080）でダッシュボードを開き、ADMIN_EMAIL / ADMIN_PASSWORD でサインインします。既定のパスワードをそのまま使った場合、コンソールが初回ログイン時に変更を要求します。",
        },
      ],
      fields: [
        {
          key: "tag",
          label: "イメージタグ",
          placeholder: "latest",
          hint: "ghcr.io/shuaizend/pingwaf のイメージタグ。'latest' は最新リリースに追従します。本番では公開バージョン（例: 0.33.0）を固定してください。",
          defaultValue: "latest",
        },
        {
          key: "adminEmail",
          label: "管理者メールアドレス",
          placeholder: "admin@pingwaf.local",
          hint: "初期投入される管理者アカウント。既定は admin@pingwaf.local です。",
          defaultValue: "admin@pingwaf.local",
        },
        {
          key: "adminPassword",
          label: "管理者パスワード",
          placeholder: "pingwaf123",
          hint: ".env ファイルに書かれます — シェル履歴には残りません。ドキュメント既定は pingwaf123 で、コンソールが初回ログイン時に変更を要求します。",
          defaultValue: "pingwaf123",
        },
        {
          key: "tlsSans",
          label: "証明書の名称（SAN）",
          placeholder: "waf.example.com",
          hint: "自己署名ダッシュボード証明書がカバーするホスト名をカンマ区切りで指定します。空欄ならコンソールへのアクセスに使う名前がカバーされます。",
          defaultValue: "",
        },
        {
          key: "httpPort",
          label: "HTTP ポート（ホスト）",
          placeholder: "80",
          hint: "プロキシされる HTTP トラフィックのホスト側ポート。80 が使用中の場合に変更してください。コンテナ内のポートは 80 のままです。",
          defaultValue: "80",
        },
        {
          key: "httpsPort",
          label: "HTTPS ポート（ホスト）",
          placeholder: "443",
          hint: "プロキシされる HTTPS トラフィックのホスト側ポート。443 が使用中の場合に変更してください。コンテナ内のポートは 443 のままです。",
          defaultValue: "443",
        },
        {
          key: "adminPort",
          label: "ダッシュボードポート（ホスト）",
          placeholder: "9080",
          hint: "ダッシュボードと REST API のホスト側ポート。Agent 用 gRPC は :9090 のままです。",
          defaultValue: "9080",
        },
      ],
      envTitle: "生成された .env",
      commandTitle: "スタックの起動",
      note: "初回ログイン：https://<ホスト>:9080 に上記の ADMIN_EMAIL / ADMIN_PASSWORD でサインイン（正式な証明書をアップロードするまで自己署名の警告を受け入れてください）。Agent は :9090 でコントロールプレーンに接続します。",
      upgradeTitle: "アップグレード",
      upgradeBody:
        ".env のタグのイメージを取得し、同じボリュームでコンテナを再作成します — 設定・データ・証明書は再起動後も保持されます。計画的なアップグレードにはタグを固定してください。",
      upgradeCommand: "docker compose pull && docker compose up -d",
      requirements: [
        "Docker Engine 24+ と Compose v2 プラグイン（docker compose。旧 docker-compose コマンドではない）。",
        "ホストの 80・443・9080・9090 ポートが空いていること — または下のフィールドで変更できます。",
        "外部データベースは不要：compose ファイルには PostgreSQL 16 コンテナが同梱され、名前付きボリュームで永続化されます。",
        "ダッシュボードは初回起動時、自己署名証明書で HTTPS を提供します。「設定 → Control plane HTTPS」から正式な証明書をアップロードするか、TLS_ENABLED=false で平文 HTTP にできます。",
      ],
      screenshot: {
        key: "installCompose",
        alt: "docker compose up -d がネットワークとボリュームを作成し、PostgreSQL と PingWAF を起動してダッシュボード URL を表示するターミナル",
        caption:
          "docker compose up -d はイメージを取得し、PostgreSQL を起動して healthy になるのを待ち、ダッシュボードを起動します。",
      },
    },
  },
  interactive: {
    id: "interactive",
    eyebrow: "対話式インストール",
    title: "あるいはインストーラーに質問させる",
    lead: "フラグなしでターミナルからスクリプトを実行すると、モード・認証情報・データベース・証明書名を順に尋ね、確定した計画を表示して確認を待ちます。",
    steps: [
      {
        title: "デプロイモードを選ぶ",
        body: "1 all-in-one、2 server、3 agent — Enter で括弧内の既定値を受け入れます。",
      },
      {
        title: "パラメーターを入力する",
        body: "agent はコントロールプレーン URL と登録キーを、server モードは管理者アカウント・PostgreSQL URL・証明書名を尋ねます。シークレットは入力中も表示されません。",
      },
      {
        title: "サマリーを確認する",
        body: "何かを書き込む前に、インストーラーは確定済みの計画（モード・バージョン・プラットフォーム・パス・systemd の状態・マスク済みのデータベース URL）を表示します。",
      },
      {
        title: "完了して確認する",
        body: "server モードは起動コマンドを表示し、agent は自動で有効化・起動されます。ダッシュボードの「ノード」でノードを確認してください。",
      },
    ],
    terminalNote:
      "対話モードにはローカルターミナルが必要です。curl | bash の場合、stdin はスクリプト自身です — 先にスクリプトをダウンロードし、bash install.sh として実行して質問に答えてください。",
    shots: [
      {
        key: "installInteractive",
        alt: "モード・管理者アカウント・PostgreSQL URL・証明書名を順に尋ね、最後にインストール計画を表示する対話式インストーラー",
        caption:
          "対話式インストーラーは順に質問し、何かを書き込む前に確定済みの計画を表示します。",
      },
      {
        key: "installSuccess",
        alt: "バイナリ・設定・データ・サービスのパスと次のステップを列挙したインストール成功バナー",
        caption:
          "正常に完了すると pingwaf.service が登録され、各ファイルの配置先が表示されます。",
      },
    ],
  },
  reference: {
    id: "reference",
    eyebrow: "フラグ早見表",
    title: "インストーラーのフラグ一覧",
    lead: "install.sh が受け付けるすべてのフラグ。それぞれに対応する PINGWAF_* 環境変数があります。",
    rows: [
      {
        flag: "--mode",
        description:
          "動作モード：all-in-one、server、agent。既定は all-in-one です。",
      },
      {
        flag: "--version",
        description:
          "指定バージョンをインストールします（先頭の v は取り除かれます）。空欄または 'latest' で最新の GitHub リリースを解決します。",
      },
      {
        flag: "--db-url",
        description:
          "server/all-in-one 用の PostgreSQL 接続文字列。既定は postgres://pingwaf:pingwaf@localhost:5432/pingwaf です。",
      },
      {
        flag: "--server-url",
        description:
          "agent モード用のコントロールプレーン gRPC URL。https:// を使ってください — gRPC は既定で TLS で提供されます。",
      },
      {
        flag: "--api-key",
        description:
          "Agent の登録トークン。コマンドラインに残さないため PINGWAF_API_KEY として渡すこともできます。",
      },
      {
        flag: "--admin-email",
        description:
          "初期投入される管理者メールアドレス（server/all-in-one）。既定は admin@pingwaf.local です。",
      },
      {
        flag: "--admin-password",
        description:
          "初期投入される管理者パスワード（server/all-in-one）。空欄なら pingwaf123 が使われ、コンソールが初回ログイン時に変更を強制します。",
      },
      {
        flag: "--tls-sans",
        description:
          "生成されるダッシュボード証明書がカバーすべきホスト名をカンマ区切りで指定します。",
      },
      {
        flag: "--no-systemd",
        description:
          "systemd ユニットの作成をスキップします — コンテナ・アップグレード・手動のプロセス管理向けです。",
      },
      {
        flag: "--interactive / --non-interactive",
        description:
          "フラグがあっても質問を強制する、または一切質問せず指定値と既定値で実行します。フラグなしのパイプ実行は非対話のままです。",
      },
      {
        flag: "--dry-run",
        description:
          "確定済みの計画（モード・バージョン・プラットフォーム・パス）を表示して、インストールせずに終了します。",
      },
    ],
    link: {
      label: "GitHub の CLI マニュアル",
      path: "https://github.com/shuaiZend/PingWAF/blob/main/docs/cli.md",
    },
  },
  troubleshooting: {
    id: "troubleshooting",
    eyebrow: "トラブルシューティング",
    title: "インストールがきれいに終わらないとき",
    lead: "サポートで最もよく見られる 4 つの失敗と、それぞれを特定するためのコマンド。",
    items: [
      {
        q: "インストーラーが PostgreSQL に接続できない",
        a: "all-in-one と server モードは初回起動前に PostgreSQL 14+ が必要です。psql が接続できない場合スクリプトは警告を出します — サーバーをインストールし、pingwaf ユーザーとデータベースを作成し（createuser / createdb）、インストーラーを再実行するか、/etc/pingwaf/pingwaf.toml を編集して systemctl restart pingwaf を実行してください。",
      },
      {
        q: "0.0.0.0:80 や :443 で bind() failed になる",
        a: "データプレーンは非特権ユーザー pingwaf として 80/443 をバインドします。これはユニットが CAP_NET_BIND_SERVICE を付与しているからこそ機能します。それでもバインドに失敗する場合は、systemctl show pingwaf -p AmbientCapabilities で確認し、drop-in で能力を追加してください：sudo systemctl edit pingwaf → [Service] AmbientCapabilities=CAP_NET_BIND_SERVICE。付与前に書かれた古いユニットは、drop-in を追加するまで失敗し続けます。",
      },
      {
        q: "バイナリがバージョン付き glibc シンボルのエラーで失敗する",
        a: "リリース成果物は Ubuntu 24.04 でビルドされており、glibc 2.39 以上が必要です。より古いディストリビューション（Debian 11、Ubuntu 20.04、CentOS/RHEL、Alpine など）ではインストーラーのスモークテストが明確なメッセージとともに停止します — 代わりにソースからビルドしてください。",
      },
      {
        q: "systemd サービスが起動しない",
        a: "journalctl -u pingwaf -e でユニットのログを確認し、/etc/pingwaf/pingwaf.toml を見直してください。リロードはありません。変更は systemctl restart pingwaf で適用し、その後 systemctl status pingwaf で確認します。",
      },
    ],
  },
  cta: {
    title: "インストール完了。次は最初のサイトを追加しましょう。",
    lead: "ドキュメントでは、サイトモデル、分散トポロジー、設定の優先順位、環境変数の完全なリファレンスを解説しています。",
    primary: { label: "ドキュメントを読む", path: "/docs/" },
    secondary: {
      label: "GitHub Releases",
      path: "https://github.com/shuaiZend/PingWAF/releases",
    },
  },
} satisfies InstallContent;

export default install;

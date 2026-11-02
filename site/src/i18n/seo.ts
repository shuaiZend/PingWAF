import type { Locale } from "./config";

/**
 * Page-level search terms.
 *
 * Keywords are deliberately small (8-12 entries) and page-specific: technical
 * identifiers (WAF, Pingora, gRPC, ACME, ...) stay untranslated, only the
 * descriptive terms follow the locale. `BaseLayout` derives the page kind from
 * the route, so every page gets a non-empty list without each page passing one.
 */
export type PageKind = "home" | "features" | "quickstart" | "install" | "docs" | "doc";

export const SEO_KEYWORDS: Record<Locale, Record<PageKind, readonly string[]>> = {
  en: {
    home: [
      "WAF",
      "Web Application Firewall",
      "Pingora",
      "pingap",
      "Rust WAF",
      "SQLi",
      "XSS",
      "RCE",
      "gRPC",
      "self-hosted security",
    ],
    features: [
      "WAF engine",
      "CC protection",
      "bot management",
      "IP access rules",
      "edge cache",
      "ACME TLS",
      "request logs",
      "agent fleet",
    ],
    quickstart: [
      "PingWAF quickstart",
      "Docker deployment",
      "PostgreSQL",
      "all-in-one mode",
      "edge agent",
      "first sign-in",
    ],
    install: [
      "PingWAF install",
      "docker compose",
      "interactive installer",
      "TLS SAN",
      "bootstrap",
      "PostgreSQL",
    ],
    docs: [
      "PingWAF docs",
      "architecture",
      "WAF engine",
      "API reference",
      "deployment",
      "test report",
    ],
    doc: ["PingWAF documentation", "WAF internals", "configuration", "gRPC control plane"],
  },
  zh: {
    home: [
      "WAF",
      "Web 应用防火墙",
      "Pingora",
      "pingap",
      "Rust WAF",
      "SQLi",
      "XSS",
      "RCE",
      "gRPC",
      "自托管安全",
    ],
    features: [
      "WAF 引擎",
      "CC 防护",
      "Bot 管理",
      "IP 访问控制",
      "边缘缓存",
      "ACME 证书",
      "请求日志",
      "Agent 集群",
    ],
    quickstart: [
      "PingWAF 快速开始",
      "Docker 部署",
      "PostgreSQL",
      "all-in-one 模式",
      "边缘 Agent",
      "首次登录",
    ],
    install: [
      "PingWAF 安装",
      "docker compose",
      "交互式安装器",
      "TLS SAN",
      "引导安装",
      "PostgreSQL",
    ],
    docs: ["PingWAF 文档", "架构", "WAF 引擎", "API 参考", "部署", "测试报告"],
    doc: ["PingWAF 文档", "WAF 实现", "配置", "gRPC 控制面"],
  },
  ja: {
    home: [
      "WAF",
      "Web アプリケーションファイアウォール",
      "Pingora",
      "pingap",
      "Rust WAF",
      "SQLi",
      "XSS",
      "RCE",
      "gRPC",
      "セルフホスト",
    ],
    features: [
      "WAF エンジン",
      "CC 対策",
      "ボット管理",
      "IP アクセス制御",
      "エッジキャッシュ",
      "ACME 証明書",
      "リクエストログ",
      "エージェント fleet",
    ],
    quickstart: [
      "PingWAF クイックスタート",
      "Docker デプロイ",
      "PostgreSQL",
      "all-in-one モード",
      "エッジエージェント",
      "初回ログイン",
    ],
    install: [
      "PingWAF インストール",
      "docker compose",
      "対話式インストーラー",
      "TLS SAN",
      "ブートストラップ",
      "PostgreSQL",
    ],
    docs: ["PingWAF ドキュメント", "アーキテクチャ", "WAF エンジン", "API リファレンス", "デプロイ", "テストレポート"],
    doc: ["PingWAF ドキュメント", "WAF の内部", "設定", "gRPC コントロールプレーン"],
  },
  es: {
    home: [
      "WAF",
      "firewall de aplicaciones web",
      "Pingora",
      "pingap",
      "Rust WAF",
      "SQLi",
      "XSS",
      "RCE",
      "gRPC",
      "seguridad autoalojada",
    ],
    features: [
      "motor WAF",
      "protección CC",
      "gestión de bots",
      "reglas de acceso IP",
      "caché en el borde",
      "TLS ACME",
      "registros de peticiones",
      "flota de agentes",
    ],
    quickstart: [
      "inicio rápido de PingWAF",
      "despliegue con Docker",
      "PostgreSQL",
      "modo all-in-one",
      "agente de borde",
      "primer inicio de sesión",
    ],
    install: [
      "instalación de PingWAF",
      "docker compose",
      "instalador interactivo",
      "TLS SAN",
      "arranque",
      "PostgreSQL",
    ],
    docs: ["documentación de PingWAF", "arquitectura", "motor WAF", "referencia de API", "despliegue", "informe de pruebas"],
    doc: ["documentación de PingWAF", "interno del WAF", "configuración", "plano de control gRPC"],
  },
  fr: {
    home: [
      "WAF",
      "pare-feu applicatif web",
      "Pingora",
      "pingap",
      "Rust WAF",
      "SQLi",
      "XSS",
      "RCE",
      "gRPC",
      "sécurité auto-hébergée",
    ],
    features: [
      "moteur WAF",
      "protection CC",
      "gestion des bots",
      "règles d'accès IP",
      "cache en périphérie",
      "TLS ACME",
      "journaux de requêtes",
      "flotte d'agents",
    ],
    quickstart: [
      "démarrage rapide PingWAF",
      "déploiement Docker",
      "PostgreSQL",
      "mode all-in-one",
      "agent de périphérie",
      "première connexion",
    ],
    install: [
      "installation de PingWAF",
      "docker compose",
      "installateur interactif",
      "TLS SAN",
      "amorçage",
      "PostgreSQL",
    ],
    docs: ["documentation PingWAF", "architecture", "moteur WAF", "référence API", "déploiement", "rapport de tests"],
    doc: ["documentation PingWAF", "fonctionnement du WAF", "configuration", "plan de contrôle gRPC"],
  },
};

/** Route → page kind, so every page gets keywords without extra props. */
export function pageKindFor(path: string): PageKind {
  if (path === "/") return "home";
  if (path === "/features/") return "features";
  if (path === "/quickstart/") return "quickstart";
  if (path === "/install/") return "install";
  if (path === "/docs/") return "docs";
  if (path.startsWith("/docs/")) return "doc";
  return "home";
}

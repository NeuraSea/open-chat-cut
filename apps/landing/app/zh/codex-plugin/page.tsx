import type { Metadata } from "next";
import { PluginPage } from "@/components/plugin-page";

export const metadata: Metadata = { title: "Codex 插件", description: "通过 25 个 revision-safe MCP 工具和 10 个视频制作 Skills，在 Codex 中编辑本地 OpenChatCut 项目。", alternates: { canonical: "/zh/codex-plugin/", languages: { en: "/codex-plugin/", "zh-CN": "/zh/codex-plugin/" } } };
export default function Page() { return <PluginPage locale="zh"/>; }

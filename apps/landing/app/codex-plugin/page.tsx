import type { Metadata } from "next";
import { PluginPage } from "@/components/plugin-page";

export const metadata: Metadata = { title: "Codex Plugin", description: "Edit local OpenChatCut projects from Codex through 25 revision-safe MCP tools and 10 focused video-production skills.", alternates: { canonical: "/codex-plugin/", languages: { en: "/codex-plugin/", "zh-CN": "/zh/codex-plugin/" } } };
export default function Page() { return <PluginPage locale="en"/>; }

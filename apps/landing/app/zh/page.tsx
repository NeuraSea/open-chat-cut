import type { Metadata } from "next";
import { LandingPage } from "@/components/landing-page";

export const metadata: Metadata = { title: "OpenChatCut — 本地优先 AI 视频编辑器", description: "审核 Agent 剪辑计划和差异，把每个结果保留在真实可编辑的本地时间线。", alternates: { canonical: "/zh/", languages: { en: "/", "zh-CN": "/zh/" } } };
export default function Page() { return <LandingPage locale="zh" />; }

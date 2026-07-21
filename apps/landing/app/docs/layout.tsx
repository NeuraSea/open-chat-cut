import type { ReactNode } from "react";
import Link from "next/link";
import { DocsLayout } from "fumadocs-ui/layouts/docs";
import { sourceEn } from "@/lib/source";

export default function Layout({ children }: { children: ReactNode }) {
	return <DocsLayout
		tree={sourceEn.getPageTree()}
		githubUrl="https://github.com/NeuraSea/open-chat-cut"
		nav={{ title: <span className="docs-brand">◇ OpenChatCut</span>, url: "/", transparentMode: "top" }}
		links={[{ text: "Product", url: "/" }, { text: "中文", url: "/zh/docs/", secondary: true }]}
		sidebar={{ prefetch: false }}
	>{children}</DocsLayout>;
}

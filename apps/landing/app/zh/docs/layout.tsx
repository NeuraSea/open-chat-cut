import type { ReactNode } from "react";
import { DocsLayout } from "fumadocs-ui/layouts/docs";
import { sourceZh } from "@/lib/source";

export default function Layout({ children }: { children: ReactNode }) {
	return <DocsLayout
		tree={sourceZh.getPageTree()}
		githubUrl="https://github.com/NeuraSea/open-chat-cut"
		nav={{ title: <span className="docs-brand">◇ OpenChatCut</span>, url: "/zh/", transparentMode: "top" }}
		links={[{ text: "产品", url: "/zh/" }, { text: "English", url: "/docs/", secondary: true }]}
		sidebar={{ prefetch: false }}
	>{children}</DocsLayout>;
}

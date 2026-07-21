import type { ReactNode } from "react";

export default function ChineseLayout({ children }: { children: ReactNode }) {
	return <>
		<script dangerouslySetInnerHTML={{ __html: "document.documentElement.lang='zh-CN'" }} />
		{children}
	</>;
}

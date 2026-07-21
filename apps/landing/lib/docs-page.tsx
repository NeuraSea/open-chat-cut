import type { Metadata } from "next";
import { notFound } from "next/navigation";
import type { ComponentProps, ComponentType } from "react";
import type { MDXComponents } from "mdx/types";
import { DocsBody, DocsDescription, DocsPage, DocsTitle } from "fumadocs-ui/layouts/docs/page";
import { getMDXComponents } from "@/components/mdx";
import { sourceEn, sourceZh } from "@/lib/source";

type DocPage = NonNullable<ReturnType<typeof sourceEn.getPage>>;

export function docsParams(locale: "en" | "zh") {
	return (locale === "en" ? sourceEn : sourceZh).generateParams();
}

export async function docsMetadata(locale: "en" | "zh", params: Promise<{ slug?: string[] }>): Promise<Metadata> {
	const source = locale === "en" ? sourceEn : sourceZh;
	const page = source.getPage((await params).slug) as DocPage | undefined;
	if (!page) notFound();
	return { title: page.data.title, description: page.data.description };
}

export async function DocsContent({ locale, params }: { locale: "en" | "zh"; params: Promise<{ slug?: string[] }> }) {
	const source = locale === "en" ? sourceEn : sourceZh;
	const page = source.getPage((await params).slug) as DocPage | undefined;
	if (!page) notFound();
	const data = page.data as typeof page.data & {
		body: ComponentType<{ components?: MDXComponents }>;
		toc: ComponentProps<typeof DocsPage>["toc"];
	};
	const MDX = data.body;
	return <DocsPage toc={data.toc} full={false} tableOfContent={{ style: "clerk" }}>
		<DocsTitle>{page.data.title}</DocsTitle>
		<DocsDescription>{page.data.description}</DocsDescription>
		<DocsBody><MDX components={getMDXComponents()} /></DocsBody>
	</DocsPage>;
}

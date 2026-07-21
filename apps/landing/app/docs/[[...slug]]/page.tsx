import type { Metadata } from "next";
import { DocsContent, docsMetadata, docsParams } from "@/lib/docs-page";

export function generateStaticParams() { return docsParams("en"); }
export function generateMetadata({ params }: { params: Promise<{ slug?: string[] }> }): Promise<Metadata> { return docsMetadata("en", params); }
export default function Page({ params }: { params: Promise<{ slug?: string[] }> }) { return <DocsContent locale="en" params={params} />; }

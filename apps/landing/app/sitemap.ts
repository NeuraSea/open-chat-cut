import type { MetadataRoute } from "next";
import { sourceEn, sourceZh } from "@/lib/source";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
	const base = "https://open-chatcut.nervafs.xyz";
	return ["/", "/zh/", ...sourceEn.getPages().map(p => `${p.url}/`), ...sourceZh.getPages().map(p => `${p.url}/`)].map(path => ({ url: `${base}${path}`.replace(/([^:]\/)\/+/, "$1"), changeFrequency: "weekly" }));
}

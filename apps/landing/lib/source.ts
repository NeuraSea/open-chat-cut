import { docsEn, docsZh } from "../.source/server";
import { loader } from "fumadocs-core/source";

export const sourceEn = loader({
	baseUrl: "/docs",
	source: docsEn.toFumadocsSource(),
});

export const sourceZh = loader({
	baseUrl: "/zh/docs",
	source: docsZh.toFumadocsSource(),
});

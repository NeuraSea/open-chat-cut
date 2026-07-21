import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

async function localizeHtml(directory) {
	for (const entry of await readdir(directory, { withFileTypes: true })) {
		const path = join(directory, entry.name);
		if (entry.isDirectory()) {
			await localizeHtml(path);
		} else if (entry.name.endsWith(".html")) {
			const html = await readFile(path, "utf8");
			await writeFile(path, html.replace('<html lang="en"', '<html lang="zh-CN"'));
		}
	}
}

await localizeHtml(fileURLToPath(new URL("../out/zh", import.meta.url)));

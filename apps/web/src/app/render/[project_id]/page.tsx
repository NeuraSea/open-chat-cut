import { HeadlessRendererClient } from "./renderer-client";

export const dynamic = "force-dynamic";

function parseNonNegativeInteger(
	value: string | string[] | undefined,
): number | null {
	const candidate = Array.isArray(value) ? value[0] : value;
	if (!candidate || !/^\d+$/u.test(candidate)) return null;
	const parsed = Number(candidate);
	return Number.isSafeInteger(parsed) ? parsed : null;
}

function parsePreviewWidth(value: string | string[] | undefined): number {
	const parsed = parseNonNegativeInteger(value);
	return parsed !== null && parsed >= 64 && parsed <= 3840 ? parsed : 1280;
}

export default async function HeadlessRendererPage({
	params,
	searchParams,
}: {
	params: Promise<{ project_id: string }>;
	searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
	const [{ project_id: projectId }, query] = await Promise.all([
		params,
		searchParams,
	]);
	const revision = parseNonNegativeInteger(query.revision);
	const previewWidth = parsePreviewWidth(query.width);

	if (revision === null) {
		return (
			<main data-openchatcut-renderer-state="error">
				Invalid or missing pinned revision.
			</main>
		);
	}

	return (
		<HeadlessRendererClient
			projectId={projectId}
			revision={revision}
			previewWidth={previewWidth}
		/>
	);
}

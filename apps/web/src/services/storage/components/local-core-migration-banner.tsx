"use client";

import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { useEditor } from "@/editor/use-editor";
import { storageService } from "../service";

export function LocalCoreMigrationBanner() {
	const editor = useEditor();
	const [legacyCount, setLegacyCount] = useState(0);
	const [isMigrating, setIsMigrating] = useState(false);

	useEffect(() => {
		let cancelled = false;
		void storageService
			.countLegacyProjects()
			.then((count) => {
				if (!cancelled) setLegacyCount(count);
			})
			.catch(() => {
				// The projects screen already reports daemon connectivity failures.
			});
		return () => {
			cancelled = true;
		};
	}, []);

	if (legacyCount === 0) return null;

	const migrate = async () => {
		setIsMigrating(true);
		try {
			const result = await storageService.migrateLegacyProjects();
			setLegacyCount(result.failed.length);
			await editor.project.loadAllProjects();
			if (result.failed.length > 0) {
				toast.warning("Some Classic projects could not be imported", {
					description: `${result.migrated} imported, ${result.failed.length} left unchanged.`,
				});
				return;
			}
			toast.success(`Imported ${result.migrated} Classic project(s)`);
		} catch (error) {
			toast.error("Classic project import failed", {
				description:
					error instanceof Error ? error.message : "Local daemon unavailable",
			});
		} finally {
			setIsMigrating(false);
		}
	};

	return (
		<section className="mx-8 mt-3 flex flex-col gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 sm:flex-row sm:items-center sm:justify-between">
			<div>
				<h2 className="font-medium">Import OpenCut Classic projects</h2>
				<p className="text-muted-foreground text-sm">
					{legacyCount} browser project(s) are not in the revisioned local core
					yet. Import copies them to SQLite; the IndexedDB originals stay
					unchanged.
				</p>
			</div>
			<Button type="button" onClick={migrate} disabled={isMigrating}>
				{isMigrating ? "Importing..." : "Import to local core"}
			</Button>
		</section>
	);
}

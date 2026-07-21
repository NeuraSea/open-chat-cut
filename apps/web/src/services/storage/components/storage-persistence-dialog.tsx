"use client";

import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogBody,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { useStoragePersistence } from "@/services/storage/use-storage-persistence";

export function StoragePersistenceDialog() {
	const { showDialog, onConfirm, onDismiss } = useStoragePersistence();

	return (
		<Dialog open={showDialog} onOpenChange={(open) => !open && onDismiss()}>
			<DialogContent className="sm:max-w-md">
				<DialogHeader>
					<DialogTitle>Keep the browser cache available</DialogTitle>
				</DialogHeader>
				<DialogBody>
					<p className="text-base text-muted-foreground">
						Your projects and managed media are already safe in the local
						daemon.
					</p>
					<p className="text-base text-muted-foreground">
						Allow OpenChatCut to retain its optional browser preview and
						migration cache when storage runs low?
					</p>
				</DialogBody>
				<DialogFooter>
					<Button variant="outline" onClick={onDismiss}>
						Not now
					</Button>
					<Button onClick={onConfirm}>Allow cache</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}

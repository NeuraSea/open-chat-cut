import type { Metadata } from "next";
import { RootProvider } from "fumadocs-ui/provider/next";
import type { ReactNode } from "react";
import "./globals.css";

export const metadata: Metadata = {
	metadataBase: new URL("https://open-chatcut.nervafs.xyz"),
	title: { default: "OpenChatCut — Local-first AI video editor", template: "%s · OpenChatCut" },
	description: "Plan edits with an agent, approve the diff, and keep every clip, caption, motion graphic, and generated asset editable.",
	openGraph: { type: "website", siteName: "OpenChatCut", images: ["/editor.png"] },
	icons: {
		icon: [
			{ url: "/favicon-v2.svg", type: "image/svg+xml" },
			{ url: "/favicon-32x32.png", type: "image/png", sizes: "32x32" },
		],
		shortcut: "/favicon.ico",
		apple: [{ url: "/apple-touch-icon.png", sizes: "180x180", type: "image/png" }],
	},
};

export default function RootLayout({ children }: { children: ReactNode }) {
	return (
		<html lang="en" suppressHydrationWarning>
			<body className="flex min-h-screen flex-col">
				<RootProvider theme={{ enabled: false }} search={{ enabled: false }}>
					{children}
				</RootProvider>
			</body>
		</html>
	);
}

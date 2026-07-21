import { z } from "zod";

const webEnvSchema = z.object({
	// Node
	NODE_ENV: z.enum(["development", "production", "test"]).default("development"),
	ANALYZE: z.string().optional(),
	NEXT_RUNTIME: z.enum(["nodejs", "edge"]).optional(),

	// Public
	NEXT_PUBLIC_SITE_URL: z.url().default("http://localhost:3000"),
	NEXT_PUBLIC_OPENCHATCUT_API_URL: z
		.union([z.url(), z.literal("same-origin")])
		.default("http://127.0.0.1:3210/api/v1"),
	NEXT_PUBLIC_MARBLE_API_URL: z.url().default("http://127.0.0.1"),

	// Server
	DATABASE_URL: z
		.string()
		.default("postgresql://disabled:disabled@127.0.0.1:9/disabled")
		.refine(
			(url) =>
				url.startsWith("postgres://") || url.startsWith("postgresql://"),
			"DATABASE_URL must be a postgres:// or postgresql:// URL",
		),

	BETTER_AUTH_SECRET: z.string().default("local-auth-disabled"),
	UPSTASH_REDIS_REST_URL: z.url().default("http://127.0.0.1:9"),
	UPSTASH_REDIS_REST_TOKEN: z.string().default("disabled"),
	MARBLE_WORKSPACE_KEY: z.string().default("disabled"),
	FREESOUND_CLIENT_ID: z.string().default(""),
	FREESOUND_API_KEY: z.string().default(""),
});

export type WebEnv = z.infer<typeof webEnvSchema>;

export const webEnv = webEnvSchema.parse(process.env);

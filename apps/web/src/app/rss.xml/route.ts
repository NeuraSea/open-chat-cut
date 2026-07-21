export async function GET() {
	return new Response("OpenChatCut has not published an RSS feed.", {
		status: 404,
		headers: { "content-type": "text/plain; charset=utf-8" },
	});
}

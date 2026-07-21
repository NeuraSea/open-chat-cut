export interface DeviceHints {
	userAgent: string;
	platform: string;
	maxTouchPoints: number;
	userAgentDataMobile?: boolean;
}

const MOBILE_USER_AGENT =
	/Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i;

export function isMobileDevice({
	userAgent,
	platform,
	maxTouchPoints,
	userAgentDataMobile,
}: DeviceHints): boolean {
	if (userAgentDataMobile === true) return true;

	// Modern iPadOS can identify itself as macOS. Touch capability distinguishes
	// it from an actual Mac without treating a narrow desktop window as mobile.
	const isIPadWithDesktopUserAgent =
		platform === "MacIntel" && maxTouchPoints > 1;

	return MOBILE_USER_AGENT.test(userAgent) || isIPadWithDesktopUserAgent;
}

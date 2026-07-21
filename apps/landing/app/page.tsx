import type { Metadata } from "next";
import { LandingPage } from "@/components/landing-page";

export const metadata: Metadata = { alternates: { canonical: "/", languages: { en: "/", "zh-CN": "/zh/" } } };
export default function Page() { return <LandingPage locale="en" />; }

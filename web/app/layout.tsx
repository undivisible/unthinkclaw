import type { Metadata, Viewport } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "unthinkclaw",
  description: "A faster, lighter, safer, richer agent runtime. Your personal AI operating system.",
  openGraph: {
    title: "unthinkclaw",
    description: "A faster, lighter, safer, richer agent runtime. Your personal AI operating system.",
    type: "website",
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#e9e7e1",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link
          href="https://fonts.googleapis.com/css2?family=Young+Serif&display=swap"
          rel="stylesheet"
        />
      </head>
      <body>{children}</body>
    </html>
  );
}

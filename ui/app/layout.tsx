import localFont from "next/font/local";
import "./globals.css";
import { Shell } from "./components/Shell";

// Inter, bundled so the app needs no internet to look right (SIL Open Font License; see fonts/Inter-LICENSE.txt).
const inter = localFont({ src: "./fonts/Inter-latin.woff2", variable: "--font-inter", weight: "100 900", display: "swap" });

export const metadata = { title: "OpenMediaProject", description: "Movie library manager" };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={inter.variable}>
      <body>
        <Shell>{children}</Shell>
      </body>
    </html>
  );
}

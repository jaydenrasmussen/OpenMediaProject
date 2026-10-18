import "./globals.css";
import { Shell } from "./components/Shell";

export const metadata = { title: "OpenMediaProject", description: "Movie library manager" };

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>
        <Shell>{children}</Shell>
      </body>
    </html>
  );
}

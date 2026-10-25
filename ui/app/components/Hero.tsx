import { imageSrc } from "../lib/api";
import { fanartUrl } from "../lib/format";
import type { Cover } from "../lib/types";

/** A wide banner: the backdrop, a big title and what to do next. */
export function Hero({
  images,
  eyebrow,
  title,
  meta,
  overview,
  children,
}: {
  images: Cover[];
  eyebrow: string;
  title: string;
  meta?: string;
  overview?: string | null;
  children?: React.ReactNode;
}) {
  const url = fanartUrl(images);
  return (
    <section className="hero">
      {url && (
        // eslint-disable-next-line @next/next/no-img-element
        <img className="hero-art" src={imageSrc(url)} alt="" />
      )}
      <div className="hero-body">
        <div className="eyebrow">{eyebrow}</div>
        <h2 className="hero-title">{title}</h2>
        {meta && <div className="hero-meta">{meta}</div>}
        {overview && <p className="clamp-3">{overview}</p>}
        {children && <div className="row">{children}</div>}
      </div>
    </section>
  );
}

/** A banner behind the top of a detail page; the page rises over its lower edge. */
export function Backdrop({ images }: { images: Cover[] }) {
  const url = fanartUrl(images);
  if (!url) return null;
  return (
    <div className="backdrop" aria-hidden>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img src={imageSrc(url)} alt="" />
    </div>
  );
}

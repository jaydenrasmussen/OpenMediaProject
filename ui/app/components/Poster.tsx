import { imageSrc } from "../lib/api";
import { posterUrl } from "../lib/format";
import type { Cover } from "../lib/types";

export function Poster({ images, title, width = 342 }: { images: Cover[]; title: string; width?: number }) {
  const url = posterUrl(images, width);
  return (
    <div className="poster">
      {url ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={imageSrc(url)} alt={`${title} poster`} loading="lazy" />
      ) : (
        <span>{title}</span>
      )}
    </div>
  );
}

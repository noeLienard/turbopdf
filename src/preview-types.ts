/** The HTML preview as the engine returns it. */
export interface HtmlFromEngine {
  style: string;
  pages: { width: number; height: number; html: string }[];
}

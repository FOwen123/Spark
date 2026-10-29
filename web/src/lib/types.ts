export type NodeKind = "frame" | "rect" | "text" | "svg" | "vector";

export type Fill =
  | { type: "solid"; color: string; hidden?: boolean }
  | { type: "linear"; angle: number; stops: { color: string; pos: number }[]; hidden?: boolean }
  | { type: "radial"; stops: { color: string; pos: number }[]; hidden?: boolean }
  | { type: "image"; asset: string; fit?: "cover" | "contain" | "fill" | "tile"; hidden?: boolean };

export type Shadow = { inset?: boolean; x: number; y: number; blur: number; spread: number; color: string; hidden?: boolean };
export type Border = { width: [number, number, number, number]; style: string; color: string };

export type Style = {
  width?: string;
  height?: string;
  position?: string;
  left?: string;
  top?: string;
  rotate?: number;
  display?: string;
  flexDirection?: string;
  flexWrap?: string;
  alignItems?: string;
  justifyContent?: string;
  gap?: string;
  padding?: [string, string, string, string];
  overflow?: string;
  opacity?: number;
  blend?: string;
  radius?: [string, string, string, string];
  fills?: Fill[];
  border?: Border;
  shadows?: Shadow[];
  blur?: number;
  backdropBlur?: number;
  fontFamily?: string;
  fontSize?: string;
  fontWeight?: number;
  fontStyle?: string;
  lineHeight?: string;
  letterSpacing?: string;
  textAlign?: string;
  color?: string;
  textDecoration?: string;
  textTransform?: string;
  raw?: Record<string, string>;
  disabled?: string[];
  [k: string]: unknown;
};

export type ExportPreset = { format: string; scale: string; suffix?: string };

export type NodeView = {
  id: string;
  kind: NodeKind;
  name: string;
  hidden?: boolean;
  locked?: boolean;
  text?: string;
  tag?: string;
  attrs?: Record<string, string>;
  style: Style;
  exports?: ExportPreset[];
  image?: { status?: string; prompt?: string; [k: string]: unknown };
  parent: string;
  key: string;
  css: Record<string, string>;
};

export type Page = { id: string; name: string; icon?: string; canvas?: string; divider?: boolean };
export type Token = { name: string; value: string; description?: string };
export type FileInfo = { id: string; name: string; path: string };
export type Rect = { x: number; y: number; w: number; h: number };
export type Snapshot = {
  file: FileInfo;
  pages: Page[];
  nodes: Record<string, NodeView>;
  rects?: Record<string, [number, number, number, number]>;
  tokens: Token[];
  page: string | null;
  selection: string[];
  canUndo: boolean;
  canRedo: boolean;
};
export type Patch = Partial<Omit<Snapshot, "nodes" | "file" | "rects">> & { nodes: Record<string, NodeView | null>; name?: string };

export type Agent = { id: string; name: string; transport: string; files: Record<string, string[]> };

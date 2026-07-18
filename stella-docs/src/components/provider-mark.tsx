/**
 * Provider logomarks for the API Providers section — stylized, first-party
 * monogram badges (NOT the providers' trademarked logos; we don't ship
 * third-party brand assets). Each mark is a rounded tile in a color that
 * nods to the provider's identity, with a monogram set in the docs font.
 * Colors are fixed (not theme tokens) so a provider reads the same in light
 * and dark; the low-alpha fill keeps them comfortable on both surfaces.
 */

const MARKS: Record<string, { label: string; color: string; glyph: string }> = {
  anthropic: { label: "Anthropic", color: "#cc785c", glyph: "A" },
  openai: { label: "OpenAI", color: "#10a37f", glyph: "O" },
  gemini: { label: "Google Gemini", color: "#4285f4", glyph: "G" },
  vertex: { label: "Google Vertex AI", color: "#669df6", glyph: "V" },
  bedrock: { label: "Amazon Bedrock", color: "#ff9900", glyph: "B" },
  xai: { label: "xAI", color: "#8a8f98", glyph: "X" },
  deepseek: { label: "DeepSeek", color: "#4d6bfe", glyph: "D" },
  zai: { label: "Z.ai", color: "#5661f0", glyph: "Z" },
  openrouter: { label: "OpenRouter", color: "#7c6ff0", glyph: "R" },
  local: { label: "Local server", color: "#7d8590", glyph: ">" },
};

export function ProviderMark({ id, size = 22 }: { id: string; size?: number }) {
  const mark = MARKS[id] ?? { label: id, color: "#7d8590", glyph: id.slice(0, 1).toUpperCase() };
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      role="img"
      aria-label={`${mark.label} mark`}
      style={{ display: "inline-block", verticalAlign: "-0.28em", marginRight: "0.45em" }}
    >
      <rect x="0.75" y="0.75" width="22.5" height="22.5" rx="6" fill={mark.color} fillOpacity="0.16" stroke={mark.color} strokeWidth="1.1" />
      <text
        x="12"
        y="16.6"
        textAnchor="middle"
        fontSize="12.5"
        fontWeight="700"
        fontFamily="var(--font-sans, ui-sans-serif, system-ui, sans-serif)"
        fill={mark.color}
      >
        {mark.glyph}
      </text>
    </svg>
  );
}

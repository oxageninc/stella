import defaultMdxComponents from "fumadocs-ui/mdx";
import type { MDXComponents } from "mdx/types";

import {
  FleetFanoutDiagram,
  HeroFlowDiagram,
  PipelineFlowDiagram,
  RecallLoopDiagram,
} from "@/components/diagrams";
import { ProviderMark } from "@/components/provider-mark";

/**
 * MDX component map. The Fumadocs defaults (callouts, tabs, cards, code
 * blocks, headings) plus Stella's own bespoke pieces: the animated inline
 * SVG diagrams and the provider logomarks — registered globally so any MDX
 * page can use them without imports.
 */
export function getMDXComponents(components?: MDXComponents): MDXComponents {
  return {
    ...defaultMdxComponents,
    HeroFlowDiagram,
    PipelineFlowDiagram,
    RecallLoopDiagram,
    FleetFanoutDiagram,
    ProviderMark,
    ...components,
  };
}

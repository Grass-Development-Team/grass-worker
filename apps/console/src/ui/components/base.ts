import { css, LitElement, unsafeCSS } from "lit";
export const sharedStyles = css`
  :host {
    box-sizing: border-box;
    font-family: var(--font-sans);
    color: var(--text);
    font-size: 14px;
    line-height: 1.5;
  }
  *,
  *::before,
  *::after {
    box-sizing: border-box;
  }
  button,
  input,
  textarea,
  select {
    font: inherit;
    color: inherit;
  }
  a {
    color: inherit;
    text-underline-offset: 4px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    cursor: not-allowed;
  }
  :focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 3px;
  }
  .actions,
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .break {
    overflow-wrap: anywhere;
  }
  [hidden] {
    display: none !important;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
`;
export function componentStyles(text: string) {
  return [sharedStyles, unsafeCSS(text)];
}
export class GwElement extends LitElement {}
export function define(name: string, constructor: CustomElementConstructor): void {
  if (!customElements.get(name)) customElements.define(name, constructor);
}

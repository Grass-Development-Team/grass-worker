# Grass Worker Console Design Specification

This specification applies to every page, layout, shared component, and feedback pattern in `apps/console`. The reference login screen establishes the visual direction: black and white, neutral grays, clear typography, and generous whitespace. Hierarchy and interaction patterns draw from shadcn and Vercel; all components are implemented with Lit.

## 1. Design principles

- Establish page hierarchy through typography, spacing, backgrounds, and subtle borders.
- Use solid black-and-white buttons for primary actions and gray, outlined, or transparent buttons for secondary actions.
- Use color to communicate explicit status rather than as general decoration. Pair status colors with text or icons.
- Shared components own styling, semantics, and interactions; business pages own data and workflows.
- Use dedicated pages for extensive settings and dialogs for brief confirmations. Security workflows with multiple steps clearly identify the current step, instructions, back navigation, and errors.

## 2. Colors

The authoritative tokens are defined in `src/ui/tokens/colors.css`. CSS variables inherit into Shadow DOM. Pages and components use semantic tokens.

| Token             | Light     | Dark      | Purpose                                         |
| ----------------- | --------- | --------- | ----------------------------------------------- |
| `--background`    | `#FFFFFF` | `#111111` | Pages, card bodies, and overlays                |
| `--surface`       | `#FAFAFA` | `#171717` | Sidebars, table headers, and secondary surfaces |
| `--control`       | `#F1F1F1` | `#242424` | Inputs, secondary buttons, and subtle emphasis  |
| `--hover`         | `#E9E9E9` | `#303030` | Control hover states                            |
| `--text`          | `#171717` | `#EDEDED` | Primary text                                    |
| `--muted`         | `#666666` | `#AAAAAA` | Supporting text                                 |
| `--subtle`        | `#767676` | `#888888` | Nonessential decoration and footers             |
| `--border`        | `#E5E5E5` | `#333333` | Regular separators and outlines                 |
| `--strong-border` | `#B9B9B9` | `#555555` | Emphasized outlines                             |
| `--primary`       | `#171717` | `#EDEDED` | Primary buttons                                 |
| `--primary-text`  | `#FFFFFF` | `#171717` | Primary button text                             |
| `--focus`         | `#171717` | `#EDEDED` | Keyboard focus                                  |

Each status has foreground and background tokens for Alerts, Badges, Toasts, and field errors:

| Status      | Light foreground / background | Dark foreground / background |
| ----------- | ----------------------------- | ---------------------------- |
| Success     | `#15803D` / `#F0FDF4`         | `#86EFAC` / `#14261A`        |
| Warning     | `#92400E` / `#FFFBEB`         | `#FCD34D` / `#2C2412`        |
| Error       | `#B91C1C` / `#FEF2F2`         | `#FCA5A5` / `#301919`        |
| Information | `#1D4ED8` / `#EFF6FF`         | `#93C5FD` / `#172439`        |

Select normal text and background combinations for WCAG AA's 4.5:1 contrast requirement. Check large text and necessary control outlines against 3:1. Subtle borders and decorative colors must not be the sole indication of status or interactivity.

## 3. Corner radii and outlines

- Regular controls, cards, and dialogs use a **10px** outer radius, represented by `--radius`.
- For outlines that fit within each other, calculate `R_inner = max(0, R_outer - d)`, where `d` is the actual distance between the outlines, including borders and padding.
- This calculation applies to visually nested outlines. Independently arranged inputs and buttons inside a card retain their own 10px radii.
- Split controls use 10px outer corners and **3px** adjoining corners, represented by `--radius-join`, with a 4px gap.
- The password field's upper-left and lower-left corners are 10px; its right corners are 3px. The visibility button uses 3px left corners and 10px right corners. Both sections have matching heights and backgrounds.
- Badges use 5px radii and thin progress tracks use 3px. Avatars, status dots, and switch thumbs are circular.
- Sheet corners touching the screen edge use a radius of 0; corners facing page content use 10px.

## 4. Typography, spacing, and dimensions

Use locally bundled Geist Variable for text and Geist Mono Variable for logs, checksums, and technical content. Fall back to system fonts when the chosen fonts are unavailable.

| Level               | Font size | Purpose                                                 |
| ------------------- | --------- | ------------------------------------------------------- |
| Supporting text     | 12px      | Hints, timestamps, table headers, and technical content |
| Body text           | 14px      | Controls, tables, and settings descriptions             |
| Prominent body text | 16px      | Login inputs and prominent actions                      |
| Small headings      | 20px      | Section headings                                        |
| Page headings       | 24–28px   | Login and regular page headings                         |
| Numeric values      | 32px      | Overview statistics                                     |

Use a 4px spacing unit: 4, 8, 12, 16, 24, 32, and 48px. Use 12–24px between neighboring controls and 24–48px between paragraphs or sections. Spacing within a group is smaller than spacing between groups. Slightly tighten heading letter spacing and use a body line height of approximately 1.5.

Control sizes:

| Size | Minimum height | Use case                                 |
| ---- | -------------- | ---------------------------------------- |
| `sm` | 32px           | Table-row actions and supporting buttons |
| `md` | 40px           | Regular inputs, buttons, and selects     |
| `lg` | 48px           | Login forms and prominent submissions    |

Icons default to 18px; brand icons are approximately 24px. Icon buttons provide a full 40px interaction area. Maintain sufficient spacing between dense actions on mobile.

## 5. Backgrounds, borders, and shadows

- Use `--background` for pages and `--surface` for sidebars and table headers.
- Inputs use `--control` with a transparent 1px border so state changes do not alter their geometry.
- Cards, tables, and menus use a 1px `--border`.
- Keyboard focus uses a 2px `--focus` outline with an offset of approximately 3px.
- Use shadows for menus, dialogs, and Toasts. Identify regular content cards through borders.
- Log viewers use a separate dark terminal surface and retain text selection, scrolling, and keyboard focus.
- Overlay backdrops use approximately 38% black in the light theme and 65% black in the dark theme.

## 6. Controls and states

Buttons provide `primary`, `secondary`, `outline`, `ghost`, and `danger` variants. Loading states show a Spinner, prevent repeated activation, and retain the action label. Link buttons preserve a real `href` and native browser navigation behavior.

Inputs provide accessible labels, descriptions, errors, read-only and disabled states, and validation constraints. Password visibility toggles preserve the value, focus, and selection. Use searchable selects for larger sets of choices and RadioGroups for small sets of mutually exclusive options.

Each control covers default, hover, pressed, keyboard focus, disabled, read-only, loading, and error states. Disabled controls use reduced opacity and behavior restrictions. Errors use field feedback and semantic colors. Placeholders do not replace labels.

Tables provide native table semantics, accessible names, empty states, and horizontally scrollable containers. Row actions use compact sizes; batch operations display individual results.

## 7. Layouts and business patterns

- Login pages use a centered content area with a maximum width of approximately 560px and no decorative outer card. Retain site branding, email sign-in, registration, password recovery, and available third-party sign-in options.
- The desktop Console sidebar is 240px wide and collapses to 76px. The top bar is 64px high, and the content area has a maximum width of 1280px.
- Below 900px, the sidebar becomes mobile navigation. Below 760px, forms with two columns become a single column. Below 600px, reduce page padding.
- Tables may scroll horizontally within their own containers. The page must not overflow horizontally.
- Extensive settings, member editing, credential rotation, certificate import, and MFA enrollment use dedicated pages with clear back navigation.
- MFA distinguishes method selection, QR-code or email instructions, and code confirmation. Support switching methods, understandable errors, and an email resend countdown.
- The setup wizard shows the current step and retains a confirmation step for the one-time node token.

## 8. Overlays and feedback

Dialogs and Sheets use the native `<dialog>` top layer and require a title. Support focus entry, Escape, a noninteractive background, closing, and focus restoration. For destructive actions, show the impact in a ConfirmDialog before sending the request.

Popovers and DropdownMenus use the native Popover API to avoid clipping by containers. Menus support keyboard navigation. Tooltips provide supplementary information rather than essential actions or their only explanation.

Show field errors near their controls and offer retries for page-loading failures. Toasts communicate operation results, deduplicate messages, and limit the number displayed at once. Success messages close after approximately 5 seconds and errors after approximately 8 seconds; both can be dismissed manually. Avoid reporting the same error from multiple business-layer locations.

## 9. Motion and theme transitions

Regular interactions use 120–180ms transitions. Switching between light and dark themes uses the **View Transition API, a circular clip, and a 300ms spring animation**:

1. Calculate the circle's center from the activation point. For keyboard activation, use the button's center.
2. Calculate a radius that covers the viewport's farthest corner.
3. Use `document.startViewTransition()` to update the root theme and saved preference.
4. Animate `clip-path: circle(...)` on `::view-transition-new(root)` using 61 spring samples.
5. Keep the duration at 300ms with a linear keyframe timeline; the circle radius follows the damped spring response.

The implementation uses this normalized spring response:

```text
f(t) = 1 - exp(-8t) × (cos(12t) + 8/12 × sin(12t))
p(t) = f(t) / f(1), 0 < t < 1
p(0) = 0, p(1) = 1
```

Allow slight overshoot while ensuring the final radius covers the viewport precisely. Disable the browser's default theme crossfade and avoid overlapping transitions.

Apply the theme directly when View Transitions are unavailable, reduced motion is requested, or the transition fails. Support light, dark, and system preferences, store them under `grass-worker.theme`, and apply the initial theme before mounting.

## 10. Web Components contracts

- Prefix elements with `gw-`.
- Regular components use Shadow DOM. Pages and business compositions may use Light DOM to retain native forms and page structure.
- Form controls use `ElementInternals` for form association, supporting FormData, constraint validation, reset, disabled states, and state restoration.
- Bind complex objects through properties. Bind boolean values through boolean attributes or properties.
- Public events use `bubbles: true` and `composed: true` when needed, such as `gw-input`, `gw-change`, `gw-page`, and `gw-close`.
- Import component CSS from the component's own `style.css` and use semantic tokens. Business pages do not override component colors or radii internally.
- Release subscriptions, listeners, connections, observers, and timers on disconnection.

In development, open `/ui` to inspect components and their states. Validate behavior in real browsers, especially form association, Shadow DOM focus, native overlays, and theme transitions.

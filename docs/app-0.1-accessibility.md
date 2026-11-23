# lrsalvage app 0.1.0 accessibility statement

The lrsalvage browser app targets WCAG 2.2 Level AA for the recovery guide
and the flow from choosing a source through saving JPEGs and reading the
receipt. Full conformance remains undetermined. [WCAG 2.2](https://www.w3.org/TR/WCAG22/#conformance)

## Candidate observations

The hosted preview of app 0.1.0 and core 0.4.0 at
`ac115df9cdc13e78e0baae5a1db360ff1f8cd95c` was exercised in Chrome
157.0.8103.79 on Windows 11 Pro 25H2, build 26200.10075.
[Pages preview](https://d79b7b5d.lrsalvage.pages.dev)

| WCAG 2.2 criterion | Observation and scope |
| --- | --- |
| 2.1.1 Keyboard | Enter opened the planned paths dialog. Keyboard input filtered the list and Escape dismissed the dialog after clearing the search. The complete recovery flow remains unchecked. |
| 2.4.3 Focus Order | Focus moved to search when the planned paths dialog opened and returned to its opener when it closed. Focus after native picks and the rest of the flow remains unchecked. |
| 2.4.6 Headings and Labels | The planned paths dialog exposed a heading, a named search field and a Close button in Chrome. Review of labels across the rest of the flow remains unchecked. |
| 4.1.2 Name, Role, Value | Chrome exposed the name, search field role and entered value for the planned paths search. End-to-end assistive technology behavior remains unchecked. |

## Unchecked on this candidate

| WCAG 2.2 criterion | Check still due |
| --- | --- |
| 1.4.3 Contrast (Minimum), 1.4.11 Non-text Contrast | Measure applicable contrast ratios and exercise focus, selection, controls and feedback in a Windows contrast theme. |
| 1.4.4 Resize Text, 1.4.10 Reflow | Use the guide, review, dialog, export controls and receipt at 200% and 400% browser zoom. Include reflow at 320 CSS pixels. |
| 2.1.1 Keyboard, 2.1.2 No Keyboard Trap, 2.4.3 Focus Order, 2.4.7 Focus Visible, 2.4.11 Focus Not Obscured (Minimum) | Complete a keyboard-only recovery in Windows Chrome and Edge. Check visible focus, picker outcomes, both save controls and both review dialogs. |
| 4.1.2 Name, Role, Value, 4.1.3 Status Messages | Use real NVDA speech to verify key control states, matching and export progress, errors and receipt announcements in Windows Chrome and Edge. |

Other applicable Level A and AA criteria remain unchecked. Earlier Windows
Chrome, Edge and NVDA observations are available for their own deployment.
[Current verification issue](https://github.com/marcd35/lrsalvage/issues/172)
[Earlier build observations](https://github.com/marcd35/lrsalvage/issues/172#issuecomment-7088260755)

## Report an accessibility barrier

Describe the step, what you expected, and what happened. If available, include
your browser and Windows versions and the assistive technology you used. Use
practice data or a synthetic example to reproduce the problem.
[Report an issue](https://github.com/marcd35/lrsalvage/issues/new)


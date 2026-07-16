---
description: Add or change an event subject in the ontology (warden-gated)
argument-hint: "[subject.name]"
---
Delegate to the warden agent to add or modify the subject named in $ARGUMENTS in `spec/ontology.md`. The warden runs the subject checklist, opens and closes the ontology session, and updates `rezidnt-types`. Direct ontology edits outside this flow are hook-blocked. If the change touches a BINDING item, the warden will route to /dr first.

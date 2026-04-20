---
project-start: 2022-02-11
project-end: 
created: 2023-08-28 07:08
updated: 2023-08-28 07:08
project:
  - "[[Orchard]]"
type: project
tags: []
---

> [! ]+
> ## General Information:
> customer:: [[Agencja Uzborjenia]]
> products:: [[ARS9000]] [[IFS]] [[CRS]] [[XRS]]
## Agenda

## Meetings
```dataview
table without id
file.link as Meeting, summary as Summary, created as Created
from !"Templates"
where contains(lower(type), "meeting") and (contains(project, this.file.name) or contains(project, this.file.link))
sort created
```
## Open Tasks
```dataview
TASK
WHERE (contains(project, this.file.name) or contains(project, this.file.link)) and !completed
```
## Completed Tasks
```dataview
TASK
WHERE (contains(project, this.file.name) or contains(project, this.file.link)) and completed
```
## Notes

```dataview
table without id
file.link as Meeting, summary as Summary, created as Created
from !"Templates"
where contains(lower(type), "note") and (contains(project, this.file.link) or contains(project, this.file.name))
sort created
```
## Quick Notes
```dataview
TABLE
rows.Details as "Details"
WHERE contains(log, this.file.name)
FLATTEN log as Details 
WHERE (contains(Details, this.file.name) or contains(Details, this.file.link))
GROUP BY file.link as Source
SORT rows.file.day desc
```

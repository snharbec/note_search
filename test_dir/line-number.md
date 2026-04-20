---
created: 2023-09-12 11:09
updated: 2023-09-12 11:09
meeting-start: 2023-09-12 11:09
meeting-end: 2023-09-12 11:09
type: meeting
tags:
project:
  - "[[TSE]]"
people:
  - "[[Jan Franzel]]"
  - "[[Andreas Eimer]]"
  - "[[Arne Ehrck]]"
  - "[[Bernd Matthiesen]]"
summary: Summary
previous-meeting:
---

## Meeting Notes

### Modem Classifier

- They have extended the ModemClassifier configuration with a Singletone modification coming from SUDAN. The sequence right now consists of 64 symbols.
- The interpretation of the matches and the skipping of parameter have been explained.
- The drawback right now is
  - When the sequence is short (below 200 symbols) the false alarm rate will grow
  - When the sequence is too long, then the effort for the correlation is high
- There are models inside inside the configuration which do no match at all, so maybe this pattern is not active any more.
- The sequence can contain the preamble and parts of the training sequence. Here the symbol skip can be used to skip data items in between.
- We demodulate absolute internally

#### Experiences from customer

- Right now most of the items they identify are based on knowledge database
- The modification of the sync sequences are not sufficient
-

#### Big Question

- How do get the optima training sequence

#### Way Forward

- We get a signal for MIL188x110_SDN and a for the SingleTone
- We try to configure the optimal training sequence

### Classification Database

When selecting

- a classifier
- success = true
  We see no result, because success is only set on the `ProofProd` classification. We should change this the following
- `ProofProd` is successful, then the the classifier before that is successful.

### AVK

AVK module for linux64 VG environment is still missing (libDavelib).

### Clover2500

- Is installed.

### OFDM 30 60BD 75HZ

- We have a pattern and a coding table. Do we have the transmission mode.
- We received 3 signals for that.

### SingleTone

Activate new license for extended configuration.

### Update Script

- [ ] Restrict this to the creation of classnames.txt
- [ ] Do something on 2026-04-12 but not on [[2026-04-14]], see [[Tasks-2026-04-14-DOIT]]

### Training

Training for main maintenance stuff, structure of system, applications, log files, log configuration, licenses for

- Jan and Paula

### Bug Fixes

- Load message from imagepreview is now working on Linux / Windows

## Tasks

- [x] Check in imagepreview changes

- [ ] Update MAC Addresses for TSE inside installation
- [x] Create jobs for Singletone ✅ 2023-09-14
- [x] Create jobs for OFDM30 ✅ 2023-09-14
- [x] Create protocol and send over to [[Ulf Fritsch]] ✅ 2023-09-14
- [x] Initiate actions for AVK

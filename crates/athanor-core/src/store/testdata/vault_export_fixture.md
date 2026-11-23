---
athanor_vault: 1
exported_at: "2026-11-22T21:00:00Z"
schema_version: 2
device_id: "device"
counts: {profile_sections: 1, domains: 1, pull_notes: 0, threads: 2, realizations: 1, correspondences: 0, sessions: 1, tending_days: 1, kindled: 1}
---

# Athanor vault

Everything this device's store holds, in stable order. Free text is verbatim;
the raw SQLite copy shared alongside it is the lossless form.

## Profile

### how_i_learn

---
kind: profile
section: "how_i_learn"
updated_at: "2026-07-12T08:00:00Z"
---

dialogue, proof-demanding

## Domains

### Thermodynamics

---
kind: domain
id: "dom-thermo"
name: "Thermodynamics"
created_at: "2026-07-12T08:00:00Z"
updated_at: "2026-07-12T08:00:00Z"
deleted_at: null
---

## Pull notes

_none_

## Threads

### what is entropy?

---
kind: thread
id: "thr-entropy"
prompt: "what is entropy?"
state: "fixed"
domain_id: "dom-thermo"
domain: "Thermodynamics"
born: "2026-07-12T08:00:00Z"
last_worked: "2026-07-14T09:20:00Z"
parent_realization_id: null
parent_thread_id: null
child_thread_ids: ["thr-child"]
created_at: "2026-07-12T08:00:00Z"
updated_at: "2026-07-12T08:00:00Z"
deleted_at: null
---

### what does this open?

---
kind: thread
id: "thr-child"
prompt: "what does this open?"
state: "volatile"
domain_id: "dom-thermo"
domain: "Thermodynamics"
born: "2026-07-14T09:12:00Z"
last_worked: null
parent_realization_id: "rz-1"
parent_thread_id: "thr-entropy"
child_thread_ids: []
created_at: "2026-07-14T09:12:00Z"
updated_at: "2026-07-14T09:12:00Z"
deleted_at: null
---

## Realizations

### 2026-07-14 · rz-1

---
kind: realization
id: "rz-1"
date: "2026-07-14T09:12:00Z"
thread_id: "thr-entropy"
child_thread_id: "thr-child"
domains: ["Thermodynamics"]
created_at: "2026-07-14T09:12:00Z"
---

Forgetting a bit always costs heat — erasure is never free.

## Correspondences

_none_

## Sessions

### 2026-07-14T09:00:00Z · philosophus/explain · ses-1

---
kind: session
id: "ses-1"
state: "closed"
mask: "philosophus"
mode: "explain"
thread_id: "thr-entropy"
thread_prompt: "what is entropy?"
started_at: "2026-07-14T09:00:00Z"
ended_at: "2026-07-14T09:20:00Z"
created_at: "2026-07-14T09:00:00Z"
updated_at: "2026-07-14T09:20:00Z"
turns: 2
---

#### Note · 2026-07-14T09:20:00Z

Learner tied Landauer's principle to forgetting; salt fixed.

#### Trace · 2026-07-14T09:20:00Z

landauer: forgetting costs heat

#### Transcript

##### ◇ learner

Forgetting costs energy, doesn't it?

##### ☿ mystagogue

It does — erasing a bit dissipates heat. Say it in your own words and we'll fix it.

## Tending

### 2026-07-14

---
kind: tending
day: "2026-07-14"
minutes: 20
thread_ids: ["thr-entropy"]
created_at: "2026-07-14T09:20:00Z"
---

## Kindling

### SALT

---
kind: kindling
passage_key: "SALT"
first_kindled_at: "2026-07-14T09:12:00Z"
source_id: "rz-1"
---

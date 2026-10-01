#!/usr/bin/env python3
"""Generate synthetic exam sittings for the benchmark.

Deterministic by construction: fixed seeds and a fixed key, so re-running
reproduces byte-identical inputs and therefore identical cycle counts. A
benchmark whose inputs drift is not a benchmark, and the whole point of
publishing these numbers is that someone else can obtain them.

    python bench/generate-sitting.py 1 10 100 400
    docker run ... bash bench/cycles.sh 1 10 100 400
"""
import json
import os
import random
import sys

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
EXAM_ID = 20260701
NUM_CHOICES = 5
NUM_QUESTIONS = 100  # a realistic national-exam section, not the 5-question demo


def make_key():
    rng = random.Random(1)
    questions = []
    for i in range(1, NUM_QUESTIONS + 1):
        if i == NUM_QUESTIONS:  # one cancelled question, as real exams have
            questions.append({"id": i, "weight": 10, "accepted": [], "cancelled": True})
            continue
        # every 17th question has two accepted answers (a post-appeal ruling)
        if i % 17 == 0:
            a = sorted(rng.sample(range(NUM_CHOICES), 2))
        else:
            a = [rng.randrange(NUM_CHOICES)]
        questions.append({"id": i, "weight": 10, "accepted": a})
    return {
        "exam_id": EXAM_ID,
        "num_choices": NUM_CHOICES,
        "cancel_policy": "full_credit",
        "questions": questions,
    }


def make_sheet(index, key):
    rng = random.Random(1_000_000 + index)
    answers = []
    for q in key["questions"]:
        r = rng.random()
        if r < 0.08:
            answers.append(None)  # blank
        elif r < 0.55 and q["accepted"]:
            answers.append(q["accepted"][0])  # correct
        else:
            answers.append(rng.randrange(NUM_CHOICES))
    return {
        "exam_id": EXAM_ID,
        "student_pseudonym": f"{index:064x}",
        "answers": answers,
    }


def main():
    sizes = [int(a) for a in sys.argv[1:]] or [1, 10, 100, 1000]
    os.makedirs(OUT, exist_ok=True)
    key = make_key()
    with open(os.path.join(OUT, "key.json"), "w") as f:
        json.dump(key, f)

    for n in sizes:
        d = os.path.join(OUT, f"sitting-{n}")
        os.makedirs(d, exist_ok=True)
        for i in range(n):
            # zero-padded so the CLI's name-sorted order is also numeric order
            with open(os.path.join(d, f"c{i:06d}.json"), "w") as f:
                json.dump(make_sheet(i, key), f)
        print(f"sitting-{n}: {n} sheets, {NUM_QUESTIONS} questions each")


if __name__ == "__main__":
    main()

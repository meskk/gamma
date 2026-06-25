# syntax=docker/dockerfile:1
#
# Gamma AI ingestion service — container image for Linux/cloud. This is the deploy
# unit for the rented GPU box (the chosen "rent cloud GPUs now, buy later" path).
# Multi-stage: dependencies are installed from the committed hashed lock into a
# venv, then copied into a slim, non-root runtime.
#
# IMPORTANT — the real GPU model is NOT in this image yet. It runs the deterministic
# heuristic analyser and the full pipeline today. Bringing the model online (prep
# plan P18) means, in this file: base the runtime on a CUDA image (e.g.
# nvidia/cuda:*-runtime), add the model framework + weights, add the model deps to
# requirements.lock, and set GAMMA_ANALYZER=model at run time. See RUNBOOK.md.
# (On a macOS box instead, Docker can't pass through Metal — there the service runs
# natively under launchd, not in this image.)

FROM python:3.12-slim AS builder
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
WORKDIR /app
COPY requirements.lock pyproject.toml README.md ./
COPY src ./src
# Pinned + hash-checked deps, then the package itself (no dependency re-resolution).
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir -r requirements.lock \
 && /opt/venv/bin/pip install --no-cache-dir . --no-deps

FROM python:3.12-slim AS runtime
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 PATH="/opt/venv/bin:$PATH"
# Run as a non-root user.
RUN useradd --create-home --uid 10001 gamma
COPY --from=builder /opt/venv /opt/venv
USER gamma
# The seam is idempotent and crash-safe (mirrors settlement_scheduler), so a
# supervisor may restart this freely. Config comes from the environment (see
# .env.example); GAMMA_OPERATOR_* are required and the process exits non-zero
# without them.
ENTRYPOINT ["python", "-m", "gamma_ingestion"]

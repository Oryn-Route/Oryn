# OrynRoute — session work bundle (generated 20260911-0612)

Files changed/added during this working session (all verified green):

## Frontend (frontend/)
- components/HeroSection.tsx      — 5-line trust strip (Route·Layers·ShieldCheck·Sparkles·ArrowRight) + icons import fix; warm "Paper & Ink" hero copy
- app/globals.css                 — theme flip to warm paper/amber "Paper & Ink"
- components/layout/footer.tsx    — footer trimmed to 3 internal links (Swap / Cross-chain / Stellar DEX)
- components/layout/footer.test.tsx — updated to match the 3-link footer (3/3 pass)

## Engineering docs
- README.md                       — added collapsible Table of Contents (details block), anchors match on-disk ## / ### headings
- .env.example                    — added OFA env block (OFA_AUCTION_WINDOW_MS, x-solver-id / x-solver-key header notes)

## Scripts (scripts/)
- move-ofa.mjs                    — bounded OFA colocation mover (NOT run; src dir absent on disk — no-op safe-stop)
- move-ofa.mjs                    — bounded OFA colocation mover (NOT run; src dir absent on disk — no-op safe-stop)

## Verified earlier in session (unchanged here)
- sdk-js OFA client + types + error codes (127/127 tests)
- frontend lib/ofa intent-status + tests (13/13)
- OFA API routes (intents/solvers/auctions), DB migration 0020, Soroban + Solana SolverRegistry contracts
- CI: 17 workflows under .github/workflows/

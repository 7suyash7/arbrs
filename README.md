# arbrs: A High-Performance, Multi-Protocol Arbitrage Solver in Rust


`arbrs` is a headless solver engine for finding and optimising multi-protocol arbitrage opportunities on $\text{EVM}$ chains. It's built to handle the complexity and performance demands of modern $\text{DeFi}$.

It does the following:
1.  Maps the entire $\text{DEX}$ market into a unified graph.
2.  Finds complex $\text{N}$-hop arbitrage cycles (up to "N" hops).
3.  Calculates precise **Net Profit** by modelling gas costs, flashloan fees, and liquidity depth.

---

## Core Features

* **Multi-Protocol Math:** Precise, variant-aware solvers for Uniswap (v2/v3), Curve (all variants), and Balancer.
* **$\text{N}$-Hop Graph Solver:** Uses a Breadth-First Search ($\text{BFS}$) with canonical deduplication to find all unique arbitrage paths.
* **Gas-Aware Cost Modeling:** Calculates $\text{Net Profit}$ by fetching live gas prices and $\text{WETH}$ conversion rates *before* optimization.
* **Liquidity Depth-Aware Scoring:** Uses a two-stage optimizer (Golden-Section + Binary Search) to find the **Max Capacity Input**, prioritizing high-volume, reliable trades.
* **Execution Payload:** The final output is an `ArbitrageSolution` struct containing a `Vec<SwapAction>`, an $\text{ABI}$-ready payload for an execution contract.

---

## Upcoming Features
* **More DEXes!**: Planning to add Uniswap v4 next, followed by Aerodrome and Camelot, along with a few other popular v2/v3 forks.
* **More Arb Strats:** Research interesting arb strategies and implement them here!

## Quick Start (Local Fork)

1.  **Run a local fork (Anvil):**
    ```bash
    anvil --fork-url <YOUR_RPC_URL> --block-time 12
    ```

2.  **Configure:** Set `FORK_RPC_URL="ws://127.0.0.1:8545"` in `main.rs`.

3.  **Run:**
    ```bash
    cargo run --release
    ```

4.  **Observe:** The engine will hydrate all pools and begin printing $\text{Net Profit}$ opportunities on new blocks:
    ```
    [!] Found 1 profitable opportunities! (Scored by Max Net Profit)
        => Top Opp: NET Profit 8.340226 WETH from 13.1011 WETH input
        => Hop 1: 13.1011 WETH -> 18.0020 TAIL @ 0x...
        => Final Hop (2): Output 2.9919 WETH
    ```
---

## Technical Deep Dive

For a full breakdown of the architecture, math, and optimization strategies, please read the accompanying blog post:
**[https://medium.com/@suyashnyn1/building-a-multi-protocol-arbitrage-bot-c8af44f2bfb9]**

#!/bin/bash
# EIP-7702 batches on an anvil fork: a fresh key gets gas and 20 dollars, then the executor's own approve + Uniswap
# swap runs against the fork (Tests/WalletTests/Batch7702Tests.swift). NET=base (default), arbitrum, ethereum or polygon: Uniswap v3,
# approve + swap; NET=robinhood: Uniswap v4, approve to Permit2 + Permit2's approve + swap.
# Nothing touches a real network but the fork's reads. Run from App/: NET=… Tools/fork7702.sh [--skip-build]
set -euo pipefail
PORT=8547
# NET, not CHAIN: Foundry reads CHAIN as cast's --chain.
NET=${NET:-base}
case $NET in
  # Tenderly's gateway: mainnet.base.org's rate limit left a cold fork minutes behind its first swaps.
  base)      FORK=${FORK_URL:-https://base.gateway.tenderly.co}; USDC=0x833589fcd6edb6e08f4c7c32d4f71b54bda02913
             QUOTER=0x3d4e44eb1374240ce5f1b871ab261cd16335b76a; WETH=0x4200000000000000000000000000000000000006
             TEST="forkApproveAndSwapGoAsOneTransaction forkDepositsGoAsOneTransaction forkBaseV4BuyIsOneTransaction forkV4SwapThroughAVouchedHook forkSwapsTokenForToken forkKyberSwapPaysThisWallet" ;;
  arbitrum)  FORK=${FORK_URL:-https://arb1.arbitrum.io/rpc}; USDC=0xaf88d065e77c8cc2239327c5edb3a432268e5831
             QUOTER=0x61ffe014ba17989e743c5f6cb21bf9697530b21e; WETH=0x82af49447d8a07e3bd95bd0d56f35241523fbab1; TEST=forkApproveAndSwapGoAsOneTransaction ;;
  ethereum)  FORK=${FORK_URL:-https://ethereum-rpc.publicnode.com}; USDC=0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
             QUOTER=0x61ffe014ba17989e743c5f6cb21bf9697530b21e; WETH=0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2; TEST="forkApproveAndSwapGoAsOneTransaction forkV2BuysAndSellsPEPE" ;;
  polygon)   FORK=${FORK_URL:-https://polygon.drpc.org}; USDC=0x3c499c542cef5e3811e1192ce70d8cc03d5c3359
             QUOTER=0x61ffe014ba17989e743c5f6cb21bf9697530b21e; WETH=0x0d500b1d8e8ef31e21c99d1db9a6444d3adf1270; TEST=forkApproveAndSwapGoAsOneTransaction ;;
  robinhood) FORK=${FORK_URL:-https://rpc.mainnet.chain.robinhood.com}; USDC=0x5fc5360d0400a0fd4f2af552add042d716f1d168; TEST=forkRobinhoodV4SwapIsOneTransaction ;;
  *) echo "NET is base, arbitrum, ethereum, polygon or robinhood"; exit 1 ;;
esac
TEST=${TESTS:-$TEST}   # TESTS="name …" runs only those
anvil --fork-url "$FORK" --port $PORT --hardfork prague --silent &
ANVIL=$!
trap 'kill $ANVIL 2>/dev/null' EXIT
RPC=http://127.0.0.1:$PORT
for _ in $(seq 1 60); do cast chain-id --rpc-url $RPC >/dev/null 2>&1 && break; sleep 0.5; done
KEY=$(cast wallet new --json | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["private_key"])')
ME=$(cast wallet address --private-key "$KEY")
cast rpc --rpc-url $RPC anvil_setBalance "$ME" 0xDE0B6B3A7640000 >/dev/null              # 1 ETH (or POL): Ethereum's gas
if [ "$NET" = robinhood ]; then
  # USDG from Uniswap v4's PoolManager, which holds the pools' USDG: impersonated on the fork only.
  PM=0x8366a39cc670b4001a1121b8f6a443a643e40951
  cast rpc --rpc-url $RPC anvil_setBalance $PM 0x2386F26FC10000 >/dev/null
  cast rpc --rpc-url $RPC anvil_impersonateAccount $PM >/dev/null
  cast send $USDC "transfer(address,uint256)" "$ME" 20000000 --from $PM --unlocked --rpc-url $RPC >/dev/null
else
  # FiatToken keeps balances in a mapping at slot 9.
  cast rpc --rpc-url $RPC anvil_setStorageAt $USDC "$(cast index address "$ME" 9)" 0x$(printf '%064x' 20000000) >/dev/null
  # Warm the fork: anvil fetches state lazily (a first quote took 60 s through drpc), longer than the app's RPC timeout.
  for FEE in 500 3000 10000 100; do
    cast call $QUOTER "quoteExactInputSingle((address,address,uint256,uint24,uint160))(uint256,uint160,uint32,uint256)" \
      "($USDC,$WETH,3000000,$FEE,0)" --rpc-url $RPC >/dev/null 2>&1 || true
  done
  if [ "$NET" = ethereum ]; then   # Uniswap v2's USDC/WETH and PEPE/WETH pairs for the v2 run
    cast call 0x7a250d5630b4cf539739df2c5dacb4c659f2488d "getAmountsOut(uint256,address[])(uint256[])" 3000000 \
      "[$USDC,$WETH,0x6982508145454ce325ddbe47a25d4ec3d2311933]" --rpc-url $RPC >/dev/null 2>&1 || true
  fi
  if [ "$NET" = base ]; then   # Aave's pool and Morpho's vault (it reads every market it lends to) for the deposits
    cast call 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5 "getReserveData(address)" $USDC --rpc-url $RPC >/dev/null 2>&1 || true
    cast call 0xee8f4ec5672f09119b96ab6fb59c27e1b7e44b61 "totalAssets()(uint256)" --rpc-url $RPC >/dev/null 2>&1 || true
    cast call 0xee8f4ec5672f09119b96ab6fb59c27e1b7e44b61 "maxDeposit(address)(uint256)" "$ME" --rpc-url $RPC >/dev/null 2>&1 || true
    # The token-for-token run's pools (WETH/AERO, USDC/WETH, WETH/DEGEN, AERO/DEGEN): a cold fork fetching them while it
    # mines took past the app's two-minute wait for a receipt.
    for PAIR in "$WETH 0x940181a94a35a4569e4529a3cdfb74e38fd98631" "$USDC $WETH" "$WETH 0x4ed4e862860bed51a9570b96d89af5e1b0efefed" \
                "0x4ed4e862860bed51a9570b96d89af5e1b0efefed 0x940181a94a35a4569e4529a3cdfb74e38fd98631"; do
      read -r A B <<< "$PAIR"   # not «set --»: $1 is this script's own argument
      for FEE in 500 3000 10000; do
        cast call $QUOTER "quoteExactInputSingle((address,address,uint256,uint24,uint160))(uint256,uint160,uint32,uint256)" \
          "($A,$B,1000000,$FEE,0)" --rpc-url $RPC >/dev/null 2>&1 || true
      done
    done
    # The PRQUANGD/WETH pool behind Doppler's hook, for the vouched-hook run
    cast call 0x0d5e0f971ed27fbff6c2837bf31316121532048d "quoteExactInputSingle(((address,address,uint24,int24,address),bool,uint128,bytes))(uint256,uint256)" \
      "((0x112f133ae3a57bcc79b21a6319fecc2e5cedd862,$WETH,40000,10,0x892d3c2b4abeaaf67d52a7b29783e2161b7cad40),false,100000000000000,0x)" --rpc-url $RPC >/dev/null 2>&1 || true
  fi
fi
echo "$NET fork $(cast block-number --rpc-url $RPC) · $ME · $(cast call $USDC 'balanceOf(address)(uint256)' "$ME" --rpc-url $RPC) units"
# One test at a time: they share the key, and parallel ones would race for its nonce. Each runs even when one before
# it failed; the exit code says whether all passed.
FAILED=0
for T in $TEST; do
  IHATECRYPTO_ANVIL=$RPC IHATECRYPTO_ANVIL_CHAIN=$(cast chain-id --rpc-url $RPC) IHATECRYPTO_ANVIL_KEY=${KEY#0x} swift test ${1:-} --filter "$T" || FAILED=1
done
exit $FAILED

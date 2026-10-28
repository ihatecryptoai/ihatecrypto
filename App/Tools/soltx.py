#!/usr/bin/env python3
"""What an unsigned Solana transaction (base64, as a protocol's API builds it) does, before any code signs it: each
instruction's program, its Anchor instruction by name where the 8-byte discriminator matches a known one, and its
accounts. Lookup-table entries print as LUT#n (the signer can't read tables either: SolanaGuard.check).
    python3 -I Tools/soltx.py <file with the base64 transaction | -> [owner address] [extra_instruction_name …]
With an owner, that address prints as OWNER. Extra names are hashed like Anchor does: sha256("global:<name>")[:8]."""
import base64, hashlib, sys
A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58(b):
    n = int.from_bytes(b, "big"); s = ""
    while n: n, r = divmod(n, 58); s = A[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\0"))) + s
def cu16(d, i):
    v = s = 0
    while True:
        b = d[i]; i += 1; v |= (b & 0x7f) << s; s += 7
        if b < 0x80: return v, i
raw = base64.b64decode((sys.stdin if sys.argv[1] == '-' else open(sys.argv[1])).read().strip())
owner = sys.argv[2] if len(sys.argv) > 2 else None
n, i = cu16(raw, 0); i += 64 * n
v0 = raw[i] & 0x80; i += 1 if v0 else 0
req, ro_s, ro_u = raw[i], raw[i+1], raw[i+2]; i += 3
k, i = cu16(raw, i); keys = [b58(raw[i+32*j:i+32*j+32]) for j in range(k)]; i += 32 * k + 32
names = {"KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD": "KLEND", "ComputeBudget111111111111111111111111111111": "CB",
         "11111111111111111111111111111111": "SYS", "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA": "TOKEN",
         "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb": "TOKEN22", "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL": "ATA",
         "AddressLookupTab1e1111111111111111111111111": "LUT", "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr": "FARMS"}
klend = {}
for nm in ["init_user_metadata","init_obligation","init_obligation_farms_for_reserve","refresh_reserve","refresh_obligation",
           "refresh_obligation_farms_for_reserve","request_elevation_group","deposit_reserve_liquidity_and_obligation_collateral",
           "deposit_reserve_liquidity_and_obligation_collateral_v2","withdraw_obligation_collateral_and_redeem_reserve_collateral",
           "withdraw_obligation_collateral_and_redeem_reserve_collateral_v2","borrow_obligation_liquidity","borrow_obligation_liquidity_v2",
           "repay_obligation_liquidity","repay_obligation_liquidity_v2","deposit_obligation_collateral","deposit_obligation_collateral_v2",
           "withdraw_obligation_collateral","withdraw_obligation_collateral_v2","redeem_reserve_collateral","repay_and_withdraw_and_redeem",
           "deposit_and_withdraw","init_referrer_token_state","flash_borrow_reserve_liquidity","flash_repay_reserve_liquidity"] + sys.argv[3:]:
    klend[hashlib.sha256(f"global:{nm}".encode()).digest()[:8].hex()] = nm
print(f"signers {req}, static keys {k}, v0 {bool(v0)}")
m, i = cu16(raw, i)
for x in range(m):
    p = raw[i]; i += 1
    a, i = cu16(raw, i); accs = list(raw[i:i+a]); i += a
    dl, i = cu16(raw, i); data = raw[i:i+dl]; i += dl
    prog = keys[p] if p < k else f"LUT#{p-k}"
    def nm(j): return ("OWNER" if keys[j] == owner else keys[j][:6]+"…") if j < k else f"LUT#{j-k}"
    disc = data[:8].hex()
    print(f"{x}: {names.get(prog, prog[:8]+'…')} {klend.get(disc, disc if len(data) >= 8 and names.get(prog) not in ('CB', 'SYS', 'TOKEN', 'TOKEN22', 'ATA', 'LUT') else data[:1].hex())} accs[{a}] " + " ".join(f"{q}:{nm(j)}" for q, j in enumerate(accs[:14])))

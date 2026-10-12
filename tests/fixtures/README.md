# Test fixtures

Dumped from **mainnet** on 2026-10-12 with the commands in
`specs/001-fund-launch-onboarding/quickstart.md`. Never replace them with devnet copies: devnet runs
different builds (Token-2022 v11.1.0; an upgradeable Squads build).

| File | What | SHA-256 of the file | SHA-256 with trailing zeros removed |
|---|---|---|---|
| `spl_token_2022.so` | Token-2022 program `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`, release `program@v11.0.0` (has permissioned burn) | `0999dbf708971e723b08d1caafc988826a59c6001ed6dc02260da07defbe1469` | `7ea94a027005b39196fa08e6d0ddcd55ec32eb43266179921a834a1ba2eb1947` |
| `squads_multisig_program.so` | Squads v4 `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf` (upgrade authority None on mainnet) | `dec8d3e0fae58c7c8f2416e5f67c25e673f047afd6dd2bba4a47e0b29a01d34c` | `d48660833989ecea3145ff726164fe640bd90696f03ce00dfd0cda258cbf2fac` |
| `squads_program_config.json` | Squads ProgramConfig account `BSTq9w3kZwNwpBXJEvTZz2G9ZTNyKBvoSeXMvwb4cNZr` | `1c07729f8d86111c8099fbadf436f7cea702f16ad24ccbca9cb6c1458d3eb203` | n/a |

`solana program dump` writes the whole program-data account, including zero padding after the
program. Release hashes (and `solana-verify`) are computed with trailing zeros removed, so check the
last column: for Token-2022 it must start `7ea94a02`, the `program@v11.0.0` release hash.

```bash
python3 -c "import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],'rb').read().rstrip(b'\0')).hexdigest())" tests/fixtures/spl_token_2022.so
```

import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { SolSwap } from "../target/types/sol_swap";
import {
  createMint,
  createAssociatedTokenAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";

describe("sol_swap", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.SolSwap as Program<SolSwap>;
  const wallet = provider.wallet as anchor.Wallet;

  // ── shared state ─────────────────────────────────────────────────────
  let mintA: anchor.web3.PublicKey;
  let mintB: anchor.web3.PublicKey;
  let lpMint: anchor.web3.Keypair;
  let tokenAReserve: anchor.web3.Keypair;
  let tokenBReserve: anchor.web3.Keypair;
  let userTokenA: anchor.web3.PublicKey;
  let userTokenB: anchor.web3.PublicKey;
  let userLpAccount: anchor.web3.PublicKey;
  let poolPda: anchor.web3.PublicKey;
  let poolAuthorityPda: anchor.web3.PublicKey;
  let poolBump: number;
  let authorityBump: number;

  const FEE_BPS = new BN(30); // 0.30%
  const INITIAL_A = new BN(1_000_000_000); // 1000 tokens (6 decimals)
  const INITIAL_B = new BN(2_000_000_000); // 2000 tokens

  before(async () => {
    // Create token mints
    mintA = await createMint(
      provider.connection,
      wallet.payer,
      wallet.publicKey,
      null,
      6
    );
    mintB = await createMint(
      provider.connection,
      wallet.payer,
      wallet.publicKey,
      null,
      6
    );

    // Derive pool PDAs
    [poolPda, poolBump] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("pool"), mintA.toBuffer(), mintB.toBuffer()],
      program.programId
    );
    [poolAuthorityPda, authorityBump] =
      anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("authority"), mintA.toBuffer(), mintB.toBuffer()],
        program.programId
      );

    // Create user token accounts
    userTokenA = await createAssociatedTokenAccount(
      provider.connection,
      wallet.payer,
      mintA,
      wallet.publicKey
    );
    userTokenB = await createAssociatedTokenAccount(
      provider.connection,
      wallet.payer,
      mintB,
      wallet.publicKey
    );

    // Mint tokens to user
    await mintTo(
      provider.connection,
      wallet.payer,
      mintA,
      userTokenA,
      wallet.payer,
      5_000_000_000 // 5000 tokens
    );
    await mintTo(
      provider.connection,
      wallet.payer,
      mintB,
      userTokenB,
      wallet.payer,
      10_000_000_000 // 10000 tokens
    );

    // Prepare keypairs for reserve accounts and LP mint
    lpMint = anchor.web3.Keypair.generate();
    tokenAReserve = anchor.web3.Keypair.generate();
    tokenBReserve = anchor.web3.Keypair.generate();
  });

  // ─────────────────────────────────────────────────────────────────────
  it("initializes the pool", async () => {
    await program.methods
      .initializePool(FEE_BPS)
      .accounts({
        payer: wallet.publicKey,
        tokenMintA: mintA,
        tokenMintB: mintB,
        pool: poolPda,
        poolAuthority: poolAuthorityPda,
        lpMint: lpMint.publicKey,
        tokenAReserve: tokenAReserve.publicKey,
        tokenBReserve: tokenBReserve.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([lpMint, tokenAReserve, tokenBReserve])
      .rpc();

    const pool = await program.account.pool.fetch(poolPda);

    assert.equal(pool.tokenMintA.toBase58(), mintA.toBase58(), "mint A matches");
    assert.equal(pool.tokenMintB.toBase58(), mintB.toBase58(), "mint B matches");
    assert.equal(pool.feeBps.toNumber(), 30, "fee is 30 bps");
    assert.equal(pool.reserveA.toNumber(), 0, "reserve A starts at 0");
    assert.equal(pool.reserveB.toNumber(), 0, "reserve B starts at 0");
    assert.equal(pool.totalLpSupply.toNumber(), 0, "LP supply starts at 0");

    console.log("Pool initialized:", poolPda.toBase58());
  });

  // ─────────────────────────────────────────────────────────────────────
  it("adds initial liquidity and mints LP tokens", async () => {
    // Create user LP token account
    userLpAccount = await createAssociatedTokenAccount(
      provider.connection,
      wallet.payer,
      lpMint.publicKey,
      wallet.publicKey
    );

    const [userPositionPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("position"),
        poolPda.toBuffer(),
        wallet.publicKey.toBuffer(),
      ],
      program.programId
    );

    const MIN_LP = new BN(1); // accept any LP on first deposit

    await program.methods
      .addLiquidity(INITIAL_A, INITIAL_B, MIN_LP)
      .accounts({
        user: wallet.publicKey,
        pool: poolPda,
        poolAuthority: poolAuthorityPda,
        userPosition: userPositionPda,
        userTokenA: userTokenA,
        userTokenB: userTokenB,
        tokenAReserve: tokenAReserve.publicKey,
        tokenBReserve: tokenBReserve.publicKey,
        lpMint: lpMint.publicKey,
        userLpAccount: userLpAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const pool = await program.account.pool.fetch(poolPda);
    const lpAccount = await getAccount(provider.connection, userLpAccount);

    // LP = sqrt(1000 * 2000) * 1e6 = sqrt(2e12) * 1e6 ≈ 1_414_213_562
    const expectedLp = Math.floor(
      Math.sqrt(INITIAL_A.toNumber() * INITIAL_B.toNumber())
    );
    const tolerance = 10;

    assert.approximately(
      pool.totalLpSupply.toNumber(),
      expectedLp,
      tolerance,
      "LP supply matches sqrt(a*b)"
    );
    assert.approximately(
      Number(lpAccount.amount),
      expectedLp,
      tolerance,
      "user received LP tokens"
    );
    assert.equal(
      pool.reserveA.toNumber(),
      INITIAL_A.toNumber(),
      "reserve A updated"
    );
    assert.equal(
      pool.reserveB.toNumber(),
      INITIAL_B.toNumber(),
      "reserve B updated"
    );

    console.log(
      `LP minted: ${pool.totalLpSupply.toNumber()} (expected ~${expectedLp})`
    );
  });

  // ─────────────────────────────────────────────────────────────────────
  it("swaps token A for token B (A→B)", async () => {
    const pool = await program.account.pool.fetch(poolPda);
    const reserveA = pool.reserveA.toNumber();
    const reserveB = pool.reserveB.toNumber();

    const amountIn = 100_000_000; // 100 tokens A
    const amountInAfterFee = Math.floor((amountIn * (10_000 - 30)) / 10_000);
    const expectedOut = Math.floor(
      (amountInAfterFee * reserveB) / (reserveA + amountInAfterFee)
    );
    const minOut = Math.floor(expectedOut * 0.99); // 1% slippage

    const userBBefore = await getAccount(provider.connection, userTokenB);

    await program.methods
      .swap(new BN(amountIn), new BN(minOut), true)
      .accounts({
        user: wallet.publicKey,
        pool: poolPda,
        poolAuthority: poolAuthorityPda,
        userTokenIn: userTokenA,
        userTokenOut: userTokenB,
        reserveIn: tokenAReserve.publicKey,
        reserveOut: tokenBReserve.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const userBAfter = await getAccount(provider.connection, userTokenB);
    const received = Number(userBAfter.amount) - Number(userBBefore.amount);

    assert.approximately(
      received,
      expectedOut,
      expectedOut * 0.001, // 0.1% tolerance for rounding
      "received correct amount of token B"
    );
    assert.isAbove(received, 0, "received some token B");

    console.log(
      `Swap A→B: in=${amountIn}, out=${received}, expected≈${expectedOut}`
    );
  });

  // ─────────────────────────────────────────────────────────────────────
  it("swaps token B for token A (B→A)", async () => {
    const pool = await program.account.pool.fetch(poolPda);
    const reserveA = pool.reserveA.toNumber();
    const reserveB = pool.reserveB.toNumber();

    const amountIn = 200_000_000; // 200 tokens B
    const amountInAfterFee = Math.floor((amountIn * (10_000 - 30)) / 10_000);
    const expectedOut = Math.floor(
      (amountInAfterFee * reserveA) / (reserveB + amountInAfterFee)
    );
    const minOut = Math.floor(expectedOut * 0.99);

    const userABefore = await getAccount(provider.connection, userTokenA);

    await program.methods
      .swap(new BN(amountIn), new BN(minOut), false)
      .accounts({
        user: wallet.publicKey,
        pool: poolPda,
        poolAuthority: poolAuthorityPda,
        userTokenIn: userTokenB,
        userTokenOut: userTokenA,
        reserveIn: tokenBReserve.publicKey,
        reserveOut: tokenAReserve.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const userAAfter = await getAccount(provider.connection, userTokenA);
    const received = Number(userAAfter.amount) - Number(userABefore.amount);

    assert.approximately(
      received,
      expectedOut,
      expectedOut * 0.001,
      "received correct amount of token A"
    );

    console.log(
      `Swap B→A: in=${amountIn}, out=${received}, expected≈${expectedOut}`
    );
  });

  // ─────────────────────────────────────────────────────────────────────
  it("rejects swap with insufficient slippage tolerance", async () => {
    const amountIn = 50_000_000;
    const impossibleMinOut = new BN(999_999_999_999); // way too high

    try {
      await program.methods
        .swap(new BN(amountIn), impossibleMinOut, true)
        .accounts({
          user: wallet.publicKey,
          pool: poolPda,
          poolAuthority: poolAuthorityPda,
          userTokenIn: userTokenA,
          userTokenOut: userTokenB,
          reserveIn: tokenAReserve.publicKey,
          reserveOut: tokenBReserve.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      assert.fail("Should have thrown SlippageExceeded");
    } catch (err: any) {
      assert.include(
        err.toString(),
        "SlippageExceeded",
        "SlippageExceeded error thrown"
      );
    }

    console.log("Slippage protection triggered correctly");
  });

  // ─────────────────────────────────────────────────────────────────────
  it("removes liquidity and returns underlying tokens", async () => {
    const lpAccount = await getAccount(provider.connection, userLpAccount);
    const lpBalance = Number(lpAccount.amount);
    const burnAmount = Math.floor(lpBalance / 2); // remove half

    const poolBefore = await program.account.pool.fetch(poolPda);
    const expectedA = Math.floor(
      (burnAmount * poolBefore.reserveA.toNumber()) /
        poolBefore.totalLpSupply.toNumber()
    );
    const expectedB = Math.floor(
      (burnAmount * poolBefore.reserveB.toNumber()) /
        poolBefore.totalLpSupply.toNumber()
    );

    const userABefore = await getAccount(provider.connection, userTokenA);
    const userBBefore = await getAccount(provider.connection, userTokenB);

    const [userPositionPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("position"),
        poolPda.toBuffer(),
        wallet.publicKey.toBuffer(),
      ],
      program.programId
    );

    await program.methods
      .removeLiquidity(
        new BN(burnAmount),
        new BN(Math.floor(expectedA * 0.99)),
        new BN(Math.floor(expectedB * 0.99))
      )
      .accounts({
        user: wallet.publicKey,
        pool: poolPda,
        poolAuthority: poolAuthorityPda,
        userPosition: userPositionPda,
        userTokenA: userTokenA,
        userTokenB: userTokenB,
        tokenAReserve: tokenAReserve.publicKey,
        tokenBReserve: tokenBReserve.publicKey,
        lpMint: lpMint.publicKey,
        userLpAccount: userLpAccount,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const userAAfter = await getAccount(provider.connection, userTokenA);
    const userBAfter = await getAccount(provider.connection, userTokenB);
    const receivedA = Number(userAAfter.amount) - Number(userABefore.amount);
    const receivedB = Number(userBAfter.amount) - Number(userBBefore.amount);

    assert.approximately(
      receivedA,
      expectedA,
      expectedA * 0.001,
      "received correct token A"
    );
    assert.approximately(
      receivedB,
      expectedB,
      expectedB * 0.001,
      "received correct token B"
    );

    const poolAfter = await program.account.pool.fetch(poolPda);
    assert.equal(
      poolAfter.totalLpSupply.toNumber(),
      poolBefore.totalLpSupply.toNumber() - burnAmount,
      "LP supply decreased correctly"
    );

    console.log(
      `Removed liquidity: burned ${burnAmount} LP, got ${receivedA} A + ${receivedB} B`
    );
  });

  // ─────────────────────────────────────────────────────────────────────
  it("verifies constant product invariant is maintained after swaps", async () => {
    const pool = await program.account.pool.fetch(poolPda);
    const k = pool.reserveA.toNumber() * pool.reserveB.toNumber();

    // k should only increase (due to fees) or stay the same
    assert.isAbove(k, 0, "k > 0 after swaps");

    // k from initial deposit: 1e9 * 2e9 = 2e18
    // After swaps k grows slightly due to fee retention
    const initialK = INITIAL_A.toNumber() * INITIAL_B.toNumber();
    assert.isAbove(k / 2, initialK / 2 * 0.9, "k roughly preserved after removing half");

    console.log(
      `Current k: ${k} (pool reserves: ${pool.reserveA.toNumber()} A, ${pool.reserveB.toNumber()} B)`
    );
  });
});

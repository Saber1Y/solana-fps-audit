import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { WagerProgram } from "../app/src/app/types/wager_program";
import { BN } from "@coral-xyz/anchor";
import { assert } from "chai";
import {
  generateSessionId,
  deriveGameSessionPDA,
  loadKeypair,
  setupTestAccounts,
  TOKEN_ID,
  getVaultTokenAccount
} from "./utils";
import { PublicKey } from "@solana/web3.js";

describe("Edge Case Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.WagerProgram as Program<WagerProgram>;
  const gameServer = loadKeypair('./tests/kps/gameserver.json');
  const user1 = loadKeypair('./tests/kps/user1.json');

  before(async () => {
    await setupTestAccounts(provider.connection, [gameServer, user1]);
  });

  it("Prevents duplicate player joins", async () => {
    const sessionId = generateSessionId();
    const betAmount = new BN(100000000);
    await program.methods
      .createGameSession(sessionId, betAmount, { winnerTakesAllOneVsOne: {} })
      .accounts({ gameServer: gameServer.publicKey })
      .signers([gameServer])
      .rpc();
    // First join should succeed
    await program.methods
      .joinUser(sessionId, 0)
      .accounts({ user: user1.publicKey })
      .signers([user1])
      .rpc();
    // Second join with same user should fail
    let failed = false;
    try {
      await program.methods
        .joinUser(sessionId, 0)
        .accounts({ user: user1.publicKey })
        .signers([user1])
        .rpc();
    } catch (e) {
      failed = true;
    }
    assert.isTrue(failed, "Duplicate join should fail");
  });

  it("Prevents session ID collisions", async () => {
    const sessionId = "DUPLICATE_ID";
    const betAmount = new BN(100000000);
    await program.methods
      .createGameSession(sessionId, betAmount, { winnerTakesAllOneVsOne: {} })
      .accounts({ gameServer: gameServer.publicKey })
      .signers([gameServer])
      .rpc();
    let failed = false;
    try {
      await program.methods
        .createGameSession(sessionId, betAmount, { winnerTakesAllOneVsOne: {} })
        .accounts({ gameServer: gameServer.publicKey })
        .signers([gameServer])
        .rpc();
    } catch (e) {
      failed = true;
    }
    assert.isTrue(failed, "Session ID collision should fail");
  });

  it("Prevents arithmetic overflow", async () => {
    const sessionId = generateSessionId();
    // Use a very large bet amount
    const betAmount = new BN("115792089237316195423570985008687907853269984665640564039457584007913129639935");
    let failed = false;
    try {
      await program.methods
        .createGameSession(sessionId, betAmount, { winnerTakesAllOneVsOne: {} })
        .accounts({ gameServer: gameServer.publicKey })
        .signers([gameServer])
        .rpc();
    } catch (e) {
      failed = true;
    }
    assert.isTrue(failed, "Arithmetic overflow should fail");
  });

  it("Prevents invalid team selection", async () => {
    const sessionId = generateSessionId();
    const betAmount = new BN(100000000);
    await program.methods
      .createGameSession(sessionId, betAmount, { winnerTakesAllOneVsOne: {} })
      .accounts({ gameServer: gameServer.publicKey })
      .signers([gameServer])
      .rpc();
    let failed = false;
    try {
      await program.methods
        .joinUser(sessionId, 2) // Invalid team
        .accounts({ user: user1.publicKey })
        .signers([user1])
        .rpc();
    } catch (e) {
      failed = true;
    }
    assert.isTrue(failed, "Invalid team selection should fail");
  });
});

use std::sync::Arc;

use alloy_primitives::Address;
use alloy_provider::Provider;
use crate::core::token::Token;
use crate::TokenLike;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{Row, Transaction};

/// A struct to represent a pool's data when loaded from the database.
#[derive(Debug, Clone)]
pub struct PoolRecord {
    pub address: Address,
    pub dex: String,
    pub tokens: Vec<Address>,
    pub fee: Option<u32>,
    pub tick_spacing: Option<i32>,
}

/// Manages all database connections and queries.
pub struct DbManager {
    pool: SqlitePool,
}

impl DbManager {
    pub async fn new(db_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new().max_connections(5).connect(db_url).await?;
        Ok(Self { pool })
    }

    pub async fn save_token<P: Provider + Send + Sync + 'static + ?Sized>(
        &self,
        token: &Token<P>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO tokens (address, symbol, decimals) VALUES (?, ?, ?)")
            .bind(token.address().to_string())
            .bind(token.symbol())
            .bind(token.decimals() as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_pool(
        &self,
        address: Address,
        dex: &str,
        tokens: &[Arc<Token<impl Provider + Send + Sync + 'static + ?Sized>>],
        fee: Option<u32>,
        tick_spacing: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let pool_id: i64 = sqlx::query("INSERT OR IGNORE INTO pools (address, chain_id, dex, fee, tick_spacing) VALUES (?, ?, ?, ?, ?); SELECT last_insert_rowid();")
            .bind(address.to_string())
            .bind(1) // Assuming chain_id 1
            .bind(dex)
            .bind(fee.map(|f| f as i64))
            .bind(tick_spacing.map(|ts| ts as i64))
            .fetch_one(&mut *tx)
            .await?
            .get(0);

        for token in tokens {
            self.save_token_in_tx(token, &mut tx).await?;
            sqlx::query("INSERT OR IGNORE INTO pool_tokens (pool_id, token_address) VALUES (?, ?)")
                .bind(pool_id)
                .bind(token.address().to_string())
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }
    
    async fn save_token_in_tx<'a, P: Provider + Send + Sync + 'static + ?Sized>(
        &self,
        token: &Token<P>,
        tx: &mut Transaction<'a, sqlx::Sqlite>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO tokens (address, symbol, decimals) VALUES (?, ?, ?)")
            .bind(token.address().to_string())
            .bind(token.symbol())
            .bind(token.decimals() as i64)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    pub async fn load_all_pools(&self) -> Result<Vec<PoolRecord>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT p.address, p.dex, p.fee, p.tick_spacing, GROUP_CONCAT(pt.token_address) as tokens
             FROM pools p
             JOIN pool_tokens pt ON p.id = pt.pool_id
             GROUP BY p.id"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::new();
        for row in rows {
            let token_addresses_str: String = row.get("tokens");
            let tokens = token_addresses_str.split(',')
                .map(|s| s.parse::<Address>().unwrap())
                .collect();

            records.push(PoolRecord {
                address: row.get::<String, _>("address").parse().unwrap(),
                dex: row.get("dex"),
                tokens,
                fee: row.get::<Option<i64>, _>("fee").map(|f| f as u32),
                tick_spacing: row.get::<Option<i64>, _>("tick_spacing").map(|ts| ts as i32),
            });
        }
        Ok(records)
    }

    /// Retrieves the last block number the bot successfully scanned.
    pub async fn get_last_seen_block(&self) -> Result<u64, sqlx::Error> {
        let row = sqlx::query("SELECT value FROM bot_state WHERE key = 'last_seen_block'")
            .fetch_one(&self.pool)
            .await?;
        let block_str: String = row.get("value");
        Ok(block_str.parse().unwrap_or(18_000_000))
    }

    /// Updates the last scanned block number in the database.
    pub async fn update_last_seen_block(&self, block_number: u64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE bot_state SET value = ? WHERE key = 'last_seen_block'")
            .bind(block_number.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
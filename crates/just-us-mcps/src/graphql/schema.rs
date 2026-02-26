use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};

use super::types::Recipe;

pub type JustfileSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn recipes(&self, ctx: &Context<'_>) -> Vec<Recipe> {
        ctx.data_unchecked::<Vec<Recipe>>().clone()
    }

    async fn recipe(&self, ctx: &Context<'_>, name: String) -> Option<Recipe> {
        ctx.data_unchecked::<Vec<Recipe>>()
            .iter()
            .find(|r| r.name == name)
            .cloned()
    }
}

pub fn build_schema(recipes: Vec<Recipe>) -> JustfileSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(recipes)
        .finish()
}

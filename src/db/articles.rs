use super::get_articles::*;
use super::select_article_by_slug::*;
use super::tags::{get_tags, Tag};
use super::user_feed::*;
use crate::db::{DbConnection, DbResult};
use crate::errors;
use crate::models::article::{
    slugify, Article, ArticleList, NewArticleData, PGArticle, TagList, UpdateArticleData,
};
use crate::models::user::{Profile, User};
use crate::schema;
use ammonia;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use diesel::sql_types::*;
use errors::Error;

#[derive(Queryable, QueryableByName)]
struct ArticleQuery {
    #[sql_type = "Text"]
    article_slug: String,
    #[sql_type = "Text"]
    article_title: String,
    #[sql_type = "Text"]
    article_description: String,
    #[sql_type = "Text"]
    article_body: String,
    #[sql_type = "Timestamptz"]
    article_creation: NaiveDateTime,
    #[sql_type = "Timestamptz"]
    article_update: NaiveDateTime,
    #[sql_type = "Text"]
    author_username: String,
    #[sql_type = "Nullable<Text>"]
    author_bio: Option<String>,
    #[sql_type = "Nullable<Text>"]
    author_image: Option<String>,
    #[sql_type = "Nullable<Array<Text>>"]
    tags: Option<Vec<String>>,
    #[sql_type = "Bool"]
    is_favorite: bool,
    #[sql_type = "Bool"]
    is_followed: bool,
    #[sql_type = "Integer"]
    favorites_count: i32,
    #[sql_type = "BigInt"]
    total_articles: i64,
}
pub fn articles(
    conn: &DbConnection,
    m_tag: Option<String>,
    m_author: Option<String>,
    m_offset: Option<i32>,
    m_limit: Option<i32>,
    m_favorited: Option<String>,
    current_user: Option<i32>,
) -> DbResult<ArticleList> {
    get_articles(
        m_limit,
        m_offset,
        current_user,
        m_tag,
        m_favorited,
        m_author,
    )
    .load(conn)
    .map_err(Into::into)
    .map(|v: Vec<ArticleQuery>| ArticleList {
        article_count: (&v).first().map(|x| x.total_articles).unwrap_or(0),
        articles: v.into_iter().map(from_article_query).collect::<Vec<_>>(),
    })
}

pub fn tags(conn: &DbConnection) -> DbResult<TagList> {
    get_tags(20)
        .get_results(conn)
        .map_err(Into::into)
        .map(|tags: Vec<Tag>| TagList(tags.into_iter().map(|tag| tag.tag).collect::<Vec<_>>()))
}

pub fn article(
    conn: &DbConnection,
    current_user: Option<i32>,
    search: String,
) -> DbResult<Article> {
    get_by_slug(conn, current_user, search)
}

pub fn create(conn: &DbConnection, article: &NewArticleData, user_id: i32) -> DbResult<Article> {
    use schema::articles::dsl::*;
    use schema::users;
    let profile: Profile = users::table
        .filter(users::id.eq(user_id))
        .get_result(conn)
        .map_err(Into::<Error>::into)
        .map(|u: User| u.to_profile(false))?;

    let pg_article: PGArticle = diesel::insert_into(articles)
        .values((
            slug.eq(slugify(&article.title)),
            title.eq(&ammonia::clean(&article.title)),
            description.eq(&ammonia::clean(&article.description)),
            body.eq(&ammonia::clean(&article.body)),
            created_at.eq(diesel::dsl::now),
            updated_at.eq(diesel::dsl::now),
            author.eq(user_id),
        ))
        .get_result(conn)
        .map_err(Into::<Error>::into)?;

    let tag_list: Vec<String> = article
        .tag_list
        .clone()
        .map(|tag_list| -> DbResult<Vec<String>> { update_tags(tag_list, pg_article.id, conn) })
        .unwrap_or(Ok(vec![]))?;

    Ok(pg_article.to_article(profile, tag_list, false))
}

fn update_tags(
    tag_list: Vec<String>,
    article_id: i32,
    conn: &DbConnection,
) -> DbResult<Vec<String>> {
    use schema::tags;
    let correct_tags = tag_list
        .iter()
        .map(|t| {
            ammonia::clean(&t)
                .trim()
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        })
        .filter(|t| t != "")
        .collect::<Vec<_>>();
    let tags = correct_tags
        .iter()
        .map(|t| tags::tag.eq(t))
        .collect::<Vec<_>>();
    diesel::insert_into(tags::table)
        .values(tags)
        .on_conflict_do_nothing()
        .execute(conn)
        .map_err(Into::<Error>::into)?;

    let ids: Vec<i32> = tags::table
        .filter(tags::tag.eq_any(&correct_tags))
        .select(tags::id)
        .get_results(conn)
        .map_err(Into::<Error>::into)?;

    use schema::article_tag_associations as atas;
    diesel::insert_into(atas::table)
        .values(
            ids.into_iter()
                .map(|tag_id| (atas::article_id.eq(article_id), atas::tag_id.eq(tag_id)))
                .collect::<Vec<_>>(),
        )
        .execute(conn)
        .map_err(Into::<Error>::into)?;

    Ok(correct_tags)
}

pub fn delete(conn: &DbConnection, user_id: i32, to_delete: String) -> DbResult<Article> {
    use schema::articles::dsl::*;
    let (author_id, art_id): (i32, i32) = articles
        .select((author, id))
        .filter(slug.eq(&to_delete))
        .get_result(conn)
        .map_err(Into::<Error>::into)?;
    if author_id != user_id {
        return Err(Error::Unauthorized);
    }
    let artcl = get_by_slug(conn, Some(user_id), to_delete)?;
    diesel::delete(articles)
        .filter(id.eq(art_id))
        .execute(conn)
        .map_err(Into::<Error>::into)?;
    Ok(artcl)
}

use schema::articles;
#[derive(AsChangeset)]
#[table_name = "articles"]
struct ChangeArticle {
    slug: Option<String>,
    title: Option<String>,
    description: Option<String>,
    body: Option<String>,
}

pub fn update(
    conn: &DbConnection,
    user_id: i32,
    to_update: String,
    data: &UpdateArticleData,
) -> DbResult<Article> {
    use schema::articles::dsl::*;
    let (art_id, art_title): (i32, String) = articles
        .filter(slug.eq(&to_update).and(author.eq(user_id)))
        .select((id, title))
        .get_result(conn)
        .optional()
        .map_err(Into::<Error>::into)
        .and_then(|art: Option<(i32, String)>| match art {
            Some(r) => Ok(r),
            None => Err(Error::Forbidden),
        })?;
    let new_slug = data.title.as_ref().and_then(|a| {
        let t = ammonia::clean(&a);
        if t == art_title {
            None
        } else {
            Some(slugify(&a))
        }
    });
    diesel::update(articles)
        .filter(id.eq(art_id))
        .set((
            ChangeArticle {
                slug: new_slug.clone(),
                title: data.title.clone().map(|a| ammonia::clean(&a)),
                description: data.description.clone().map(|a| ammonia::clean(&a)),
                body: data.body.clone().map(|a| ammonia::clean(&a)),
            },
            updated_at.eq(diesel::dsl::now),
        ))
        .execute(conn)
        .map_err(Into::<Error>::into)?;

    data.tag_list
        .clone()
        .map(|tag_list| -> DbResult<Vec<String>> { update_tags(tag_list, art_id, conn) });
    get_by_slug(conn, Some(user_id), new_slug.unwrap_or(to_update))
}

pub fn favorite(conn: &DbConnection, favoriter: i32, fav: &String) -> DbResult<Article> {
    let mut art = get_by_slug(conn, Some(favoriter), fav.clone())?;
    let a_id: i32 = articles::table
        .filter(articles::slug.eq(fav))
        .select(articles::id)
        .first(conn)
        .map_err(Into::<Error>::into)?;
    use schema::favorites::dsl::*;
    diesel::insert_into(favorites)
        .values((user_id.eq(favoriter), article_id.eq(a_id)))
        .execute(conn)
        .map_err(Into::<Error>::into)?;
    art.favorites_count += 1;
    art.favorited = true;
    let updated = diesel::update(articles::table)
        .filter(articles::id.eq(a_id))
        .set(articles::favorites_count.eq(art.favorites_count))
        .execute(conn)?;
    if updated == 1 {
        Ok(art)
    } else {
        Err(Error::InternalServerError(
            "article".to_owned(),
            "favorites_count update failed".to_owned(),
        ))
    }
}

pub fn unfavorite(conn: &DbConnection, favoriter: i32, fav: &String) -> DbResult<Article> {
    let mut art = get_by_slug(conn, Some(favoriter), fav.clone())?;
    use schema::favorites::dsl::*;
    let a_id: i32 = articles::table
        .filter(articles::slug.eq(fav))
        .select(articles::id)
        .first(conn)
        .map_err(Into::<Error>::into)?;
    let deleted = diesel::delete(favorites)
        .filter(user_id.eq(favoriter).and(article_id.eq(a_id)))
        .execute(conn)
        .map_err(Into::<Error>::into)?;
    if deleted > 0 {
        art.favorites_count -= 1;
        art.favorited = false;
        let updated = diesel::update(articles::table)
            .filter(articles::id.eq(a_id))
            .set(articles::favorites_count.eq(art.favorites_count))
            .execute(conn)?;
        if updated == 1 {
            Ok(art)
        } else {
            Err(Error::InternalServerError(
                "article".to_owned(),
                "favorites_count update failed".to_owned(),
            ))
        }
    } else {
        Err(Error::InternalServerError(
            "article".to_owned(),
            "wasn't favorited".to_owned(),
        ))
    }
}

pub fn user_feed(
    conn: &DbConnection,
    user_id: i32,
    limit: Option<i32>,
    offset: Option<i32>,
) -> DbResult<ArticleList> {
    user_feed_query(limit, offset, user_id)
        .get_results::<ArticleQuery>(conn)
        .map(|v| ArticleList {
            article_count: (&v).first().map(|x| x.total_articles).unwrap_or(0),
            articles: v
                .into_iter()
                .map(from_article_query)
                .collect::<Vec<Article>>(),
        })
        .map_err(Into::<Error>::into)
}

fn get_by_slug(
    conn: &DbConnection,
    current_user: Option<i32>,
    search: String,
) -> DbResult<Article> {
    select_article_by_slug(current_user, search)
        .get_result::<ArticleQuery>(conn)
        .map_err(Into::into)
        .map(from_article_query)
}

fn from_article_query(aq: ArticleQuery) -> Article {
    Article {
        author: Profile {
            username: aq.author_username,
            following: aq.is_followed,
            bio: aq.author_bio,
            image: aq.author_image,
        },
        title: aq.article_title,
        body: aq.article_body,
        description: aq.article_description,
        slug: aq.article_slug,
        created_at: format! {"{:}", aq.article_creation},
        updated_at: format!["{:?}", aq.article_update],
        tag_list: aq.tags.unwrap_or(vec![]),
        favorited: aq.is_favorite,
        favorites_count: aq.favorites_count,
    }
}

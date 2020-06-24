CREATE OR REPLACE FUNCTION get_articles (
	a_limit INTEGER,
	a_offset INTEGER, 
	maybe_user_id INTEGER = NULL,
	maybe_tag TEXT = NULL,
	maybe_favorited TEXT = NULL,
	maybe_author TEXT = NULL) 
RETURNS TABLE (
	article_slug TEXT,
	article_title TEXT,
	article_description TEXT,
	article_body TEXT,
	article_creation TIMESTAMP WITH TIME ZONE,
	article_update TIMESTAMP WITH TIME ZONE,
	author_username TEXT,
	author_bio TEXT,
	author_image TEXT,
	tags TEXT[],
	is_favorite BOOL,
	is_followed BOOL ,
    favorites_count INTEGER
) 
AS $$
DECLARE 
	follow_q TEXT = '';
	favorite_q TEXT = '';
	fav_result TEXT = 'false';
	fol_result TEXT = 'false';
	where_clause TEXT = 'WHERE 1 = 1';
	where_tags TEXT = '';
BEGIN

if maybe_user_id is not null then
	follow_q := 'and followings.follower_id = ' || maybe_user_id;
	favorite_q := 'left join favorites on favorites.article_id = articles.id and favorites.user_id = ' || maybe_user_id;
	fav_result := 'count(favorites.user_id) > 0';
	fol_result := 'count(followings) > 0';
end if;

if maybe_tag is not null then
	where_tags :=  ' WHERE results.tags @> Array[''' || maybe_tag || ''']';
end if;

if maybe_author is not null then
	where_clause := where_clause || ' and users.username = ''' || maybe_author || '''';
end if;

if maybe_favorited is not null then
	where_clause := where_clause || ' and favorites.user_id = (SELECT id FROM users WHERE username = ''' || maybe_favorited ||  ''' LIMIT 1) ';
end if;

RETURN QUERY EXECUTE
'WITH results AS (
	select articles.slug,
		articles.title,
		articles.description,
		articles.body,
		articles.created_at,
		articles.updated_at,
		users.username, 
		users.bio,
		users.image,
		array_agg(tags.tag) FILTER (WHERE tags.tag is not null) as tags,
		' || fav_result || ' as is_favorite, 
		' || fol_result || ' as is_followed,
        articles.favorites_count
	from articles
	inner join users on users.id = articles.author
	left join article_tag_associations as atas on atas.article_id = articles.id
	left join tags on atas.tag_id = tags.id
	' || favorite_q || '
	left join followings on followings.followed_id = articles.author ' || follow_q || '
	' || where_clause ||'
	group by articles.id, users.id)
SELECT * 
FROM results '
|| where_tags || '
ORDER BY results.created_at DESC
LIMIT ' || a_limit || 
' OFFSET ' || a_offset || ';';

END; 
$$ LANGUAGE 'plpgsql';
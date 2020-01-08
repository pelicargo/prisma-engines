use crate::*;
use barrel::types;
use test_harness::*;

#[test_one_connector(connector = "sqlite")]
#[test]
async fn introspecting_a_simple_table_with_gql_types_must_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("Blog", |t| {
                t.add_column("bool", types::boolean());
                t.add_column("float", types::float());
                t.add_column("date", types::date());
                t.add_column("id", types::primary());
                t.add_column("int", types::integer());
                t.add_column("string", types::text());
            });
        })
        .await;
    let dm = r#"
            model Blog {
                bool    Boolean
                date    DateTime
                float   Float
                id      Int @id
                int     Int 
                string  String
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_compound_primary_keys_must_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("Blog", |t| {
                t.add_column("id", types::integer());
                t.add_column("authorId", types::text());
                t.inject_custom("PRIMARY KEY (\"id\", \"authorId\")");
            });
        })
        .await;

    let dm = r#"
            model Blog {
                authorId String
                id Int
                @@id([id, authorId])
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_unique_index_must_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("Blog", |t| {
                t.add_column("id", types::primary());
                t.add_column("authorId", types::text());
            });
            migration.inject_custom(format!(
                "Create Unique Index \"{}\".\"test\" on \"Blog\"( \"authorId\")",
                api.schema_name()
            ));
        })
        .await;

    let dm = r#"
            model Blog {
                authorId String @unique
                id Int @id
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_multi_column_unique_index_must_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("id", types::primary());
                t.add_column("firstname", types::text());
                t.add_column("lastname", types::text());
            });
            migration.inject_custom(format!(
                "Create Unique Index \"{}\".\"test\" on \"User\"( \"firstname\", \"lastname\")",
                api.schema_name()
            ));
        })
        .await;

    let dm = r#"
            model User {
                firstname String
                id Int @id
                lastname String
                @@unique([firstname, lastname], name: "test")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_required_and_optional_columns_must_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("id", types::primary());
                t.add_column("requiredname", types::text().nullable(false));
                t.add_column("optionalname", types::text().nullable(true));
            });
        })
        .await;

    let dm = r#"
            model User {
                id Int @id
                optionalname String?
                requiredname String
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

//#[test_one_connector(connector = "sqlite")]
//#[ignore]
//fn introspecting_a_table_with_datetime_default_values_should_work(api: &TestApi) {
//    let barrel = api.barrel();
//    let _setup_schema = barrel.execute(|migration| {
//        migration.create_table("User", |t| {
//            t.add_column("id", types::primary());
//            t.add_column("name", types::text());
//            t.inject_custom("\"joined\" date DEFAULT CURRENT_DATE")
//        });
//    });
//
//    let dm = r#"
//            model User {
//                id Int @id
//                joined DateTime? @default(now())
//                name String
//            }
//        "#;
//    let result = dbg!(api.introspect().await);
//    custom_assert(&result, dm);
//}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_default_values_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("a", types::text());
                t.add_column("id", types::primary());
                t.inject_custom("\"bool\" Boolean NOT NULL DEFAULT false");
                t.inject_custom("\"bool2\" Boolean NOT NULL DEFAULT 0");
                t.inject_custom("\"float\" Float NOT NULL DEFAULT 5.3");
                t.inject_custom("\"int\" INTEGER NOT NULL DEFAULT 5");
                t.inject_custom("\"string\" TEXT NOT NULL DEFAULT \"Test\"");
            });
        })
        .await;

    let dm = r#"
            model User {
                a String
                bool Boolean @default(false)
                bool2 Boolean @default(false)
                float Float @default(5.3)
                id Int @id
                int Int @default(5)
                string String @default("Test")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_a_non_unique_index_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("a", types::text());
                t.add_column("id", types::primary());
            });
            migration.inject_custom(format!(
                "Create Index \"{}\".\"test\" on \"User\"(\"a\")",
                api.schema_name()
            ));
        })
        .await;

    let dm = r#"
            model User {
                a String
                id Int @id
                @@index([a], name: "test")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "sqlite")]
async fn introspecting_a_table_with_a_multi_column_non_unique_index_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("a", types::text());
                t.add_column("b", types::text());
                t.add_column("id", types::primary());
            });
            migration.inject_custom(format!(
                "Create Index \"{}\".\"test\" on \"User\"(\"a\",\"b\")",
                api.schema_name()
            ));
        })
        .await;

    let dm = r#"
            model User {
                a String
                b String
                id Int @id
                @@index([a,b], name: "test")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

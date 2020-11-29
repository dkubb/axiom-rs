# Axiom (in Rust)

This is a project to port [axiom](https://github.com/dkubb/axiom) from ruby to rust.

## History

The [axiom](https://github.com/dkubb/axiom) project was a ruby library I wrote several years ago when first learning about relational algebra via Chris Date's books such as [Database in Depth](https://books.google.ca/books?id=TR8f5dtnC9IC) and later [SQL and Relational Theory](https://books.google.ca/books?id=WuZGD5tBfMwC).

During the process I learned a great deal about more than Relational Theory such as:

* Mutation Testing
* API Design
* Functional design
* Immutablility
* Database optimizers
* SQL Generation
* Fuzz Testing

The original motivation was to produce a lower level query builder for DataMapper. DM was different from other ORMs in that it was designed around CRUD operations and was not specifically tied to one database or any RDBMS in particular. Its true that most users used it with either MySQL or PostgreSQL but there were many third party database adapters for NoSQL databases or even REST apis.

Most existing libraries were tightly integrated with another ORM and designed around producing SQL. We needed something a bit less specialized that could work with more storage engines.

## Axiom Design

The design of Axiom (ruby) was interesting in that it built up an AST representing a query, and this AST could be used to either process in-memory objects or reflected upon to produce SQL output. An [optimizer](https://github.com/dkubb/axiom-optimizer) was created that could rewrite the AST to a simpler form that would produce the same result given the same input data, and an equivalent SQL query. The final design was relatively simple in that there was a base Relation class, and then one subclass for each kind of Relational Algebra operation such as Projection, Restriction, Union, Join and so on. The relation has methods that would output an instance of one of these subclasses that wraps the original(s). Here is a simplified example:

```ruby
# Equivalent to: SELECT id, name FROM relation
projection = Projection.new(relation, %i[id name])
```

In the above example, we have an existing relation and we project the id and name; this means we drop all other attribute from the relation besides id and name. The operation was not actually applied to the underlying data until an Enumerable method like `#each` or `#to_a` was called. The resulting relation contains enough information to reflect upon and produce an equivalent SQL query.

## Rust Port

I am just learning Rust and I need a project to actualy practice with. To learn Rust more quickly I plan to use a domain that I understand well, and I figure since I've been involved in ORM design and Relational Algebra for over a decade it would be a good choice.

I do not know what I will be able to port to Rust. Maybe none of this makes sense, maybe it can be 1:1, I'm not sure. I do know that I'd like to make something with a few capabilities:

* Must be able to process data in-memory
* Must be able to be reflected upon to generate SQL
* Must be composable
* Must have an interface that is natural for a rust developer

I'm no longer really interested in supporting a wide range of database engines. These days I mostly only choose to use PosgreSQL for an RDBMS in personal projects. I occasionally use MySQL for work, but since I'm not using Rust for anything other than personal projects I am fine to only target pg right now. I think if the focus is on relational algebra it should be possible to port to other databases, but I'm not too concerned about that right now.

In the original axiom I made use of patterns that were idiomatic in ruby, such as making sure the relation is Enumerable. I'd like to do the same thing in this project so that a relation can be an Iterator, for example.

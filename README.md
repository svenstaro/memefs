# memefs - A filesystem for your memes [![GitHub Actions Workflow](https://github.com/svenstaro/memefs/workflows/CI/badge.svg)](https://github.com/svenstaro/memefs/actions)

**Mount your memes using FUSE**

> Every meme is a file in unix

## How to get memes into your unix

    mkdir memes
    target/debug/memefs memes
    ls memes
    > 'Format with some great potential. Invest right away!.jpg'  'Oof my metal beam.jpg'
    > 'Found on me_irl thought it belonged here..png'             "Supreme Leader Snoke's Lair..jpg"
    > 'I declare these Fry Cook Games open!.jpg'                  'That escalated quickly..jpg'
    feh memes/'Oof my metal beam.jpg'

## What this does

**memefs** will look at a given subreddit or multi and fetch a bunch of hot-sorted media posts.
It then exposes them to the user as a filesystem.
It also runs a background job to refresh the memes in order to ensure that your memes stay as dank as possible.
So, in theory, this could have some useful use-cases such as fetching posts in a wallpapers subreddit.

## Change your memes

This "software" has a few "sensible" defaults. Check the defaults in the help:

    memefs --help

If you require different memes, you can do

    memefs -s https://www.reddit.com/r/prequelmemes memes

Likewise, if require more memes at the same time, you can increase the limit:

    memefs -l 50 memes

You can also refresh your memes every 60 seconds instead of 600 seconds:

    memefs -r 60 memes

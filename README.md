# memefs
Mount your memes using FUSE

> Every meme is file in unix

## How to get memes into your unix

    mkdir memes
    target/debug/memefs memes
    ls memes
    > 'Format with some great potential. Invest right away!.jpg'  'Oof my metal beam.jpg'
    > 'Found on me_irl thought it belonged here..png'             "Supreme Leader Snoke's Lair..jpg"
    > 'I declare these Fry Cook Games open!.jpg'                  'That escalated quickly..jpg'
    feh memes/'Oof my metal beam.jpg'

## Options

This "software" has a few "sensible" defaults. If you want to mix things up, see below.

If you require different memes, you can do

    target/debug/memefs -s https://www.reddit.com/r/prequelmemes memes
    
Likewise, if require very many memes at the same time, you can increase the limit:

    target/debug/memefs -l 50 memes

You can also refresh your memes every 60 seconds instead of 600 seconds:

    target/debug/memefs -r 60 memes

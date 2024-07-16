const { PurgeCSSPlugin } = require("purgecss-webpack-plugin");

const glob = require("glob");
const miniCssExtractPlugin = require('mini-css-extract-plugin');
const path = require('path');

module.exports = {
    entry: './client/app.js',
    module: {
        rules: [
            {
                test: /\.scss$/,
                use: [
                    miniCssExtractPlugin.loader,
                    'css-loader',
                    'sass-loader'
                ]
            }

        ]
    },
    output: {
        clean: true,
        filename: 'app.js',
        path: path.resolve(__dirname, 'dist')
    },
    plugins: [
        new miniCssExtractPlugin({
            filename: 'app.css'
        }),
        new PurgeCSSPlugin({
            keyframes: true,
            paths: glob.sync(path.join(__dirname, 'templates') + '/**/*', { nodir: true }),
            variables: true
        })
    ]
}
